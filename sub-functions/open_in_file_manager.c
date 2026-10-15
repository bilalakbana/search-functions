/* open_in_file_manager.c
 *   compile:  gcc -Wall -Wextra -O2 open_in_file_manager.c -o open_in_file_manager
 *   usage  :  ./open_in_file_manager /path/to/file-or-directory
 *
 * Opens the default file manager (xdg-mime default for inode/directory) on the given path,
 * selecting the file where the file manager supports it. Programs are started with fork+execvp
 * (never through a shell), so nothing in the path is ever interpreted.                      */

#define _GNU_SOURCE
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <strings.h>
#include <libgen.h>
#include <unistd.h>
#include <limits.h>
#include <fcntl.h>
#include <sys/types.h>
#include <sys/wait.h>

#define MAX_LINE 1024

/* Point stdin/stdout/stderr of the current process at /dev/null. */
static void quiet_stdio(void) {
    int fd = open("/dev/null", O_RDWR);
    if (fd < 0) return;
    dup2(fd, STDIN_FILENO);
    dup2(fd, STDOUT_FILENO);
    dup2(fd, STDERR_FILENO);
    if (fd > STDERR_FILENO) close(fd);
}

/* Start argv detached (own session, no terminal, stdio to /dev/null); 0 on success. */
static int spawn_detached(char *const argv[]) {
    pid_t pid = fork();
    if (pid < 0) { perror("fork"); return -1; }
    if (pid == 0) {
        if (setsid() < 0) _exit(127);
        pid_t grandchild = fork();
        if (grandchild < 0) _exit(127);
        if (grandchild == 0) {
            quiet_stdio();
            execvp(argv[0], argv);
            _exit(127);
        }
        _exit(0);                       /* the grandchild is reparented to init */
    }
    int status;
    if (waitpid(pid, &status, 0) < 0) { perror("waitpid"); return -1; }
    return (WIFEXITED(status) && WEXITSTATUS(status) == 0) ? 0 : -1;
}

/* Run argv to completion with stdio to /dev/null; return its exit status, -1 on failure. */
static int run_quiet(char *const argv[]) {
    pid_t pid = fork();
    if (pid < 0) return -1;
    if (pid == 0) {
        quiet_stdio();
        execvp(argv[0], argv);
        _exit(127);
    }
    int status;
    if (waitpid(pid, &status, 0) < 0) return -1;
    return WIFEXITED(status) ? WEXITSTATUS(status) : -1;
}

/* Extract the executable name from an "Exec=..." line (bounded to the output buffer). */
static void extract_exec(const char *line, char *out, size_t out_size) {
    out[0] = '\0';
    const char *p = strchr(line, '=');
    if (!p) return;
    p++;
    while (*p == ' ') p++;
    size_t n = strcspn(p, " \t\r\n");
    if (n >= out_size) n = out_size - 1;
    memcpy(out, p, n);
    out[n] = '\0';
}

/* Flatpak app installed check (flatpak info exits 0 when the app is installed). */
static int is_flatpak_installed(const char *app_id) {
    char *argv[] = { "flatpak", "info", (char *)app_id, NULL };
    return run_quiet(argv) == 0;
}

/* Open <dir>/applications/<name> for the first match in XDG_DATA_HOME then XDG_DATA_DIRS. */
static FILE *open_desktop_file(const char *name, char *path_out, size_t path_size) {
    char dirs[4096];
    const char *home = getenv("HOME");
    const char *data_home = getenv("XDG_DATA_HOME");
    const char *data_dirs = getenv("XDG_DATA_DIRS");
    int len;

    if (data_home && *data_home)
        len = snprintf(dirs, sizeof(dirs), "%s", data_home);
    else if (home && *home)
        len = snprintf(dirs, sizeof(dirs), "%s/.local/share", home);
    else
        len = snprintf(dirs, sizeof(dirs), "%s", "");
    if (len < 0 || (size_t)len >= sizeof(dirs)) return NULL;

    if (!data_dirs || !*data_dirs)
        data_dirs = "/usr/local/share:/usr/share";
    snprintf(dirs + len, sizeof(dirs) - (size_t)len, "%s%s", len ? ":" : "", data_dirs);

    char *save = NULL;
    for (char *d = strtok_r(dirs, ":", &save); d; d = strtok_r(NULL, ":", &save)) {
        int n = snprintf(path_out, path_size, "%s/applications/%s", d, name);
        if (n < 0 || (size_t)n >= path_size) continue;
        FILE *f = fopen(path_out, "r");
        if (f) return f;
    }
    return NULL;
}

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s /path/to/file\n", argv[0]);
        return 1;
    }

    char *filepath = argv[1];
    char desktop_name[256];
    FILE *fp = popen("xdg-mime query default inode/directory", "r");
    if (!fp || !fgets(desktop_name, sizeof(desktop_name), fp)) {
        if (fp) pclose(fp);
        fprintf(stderr, "Failed to get default file manager\n");
        return 1;
    }
    pclose(fp);
    desktop_name[strcspn(desktop_name, "\n")] = 0;
    if (desktop_name[0] == '\0') {
        fprintf(stderr, "No default file manager set for inode/directory\n");
        return 1;
    }

    // Open .desktop file
    char desktop_path[PATH_MAX];
    FILE *df = open_desktop_file(desktop_name, desktop_path, sizeof(desktop_path));
    if (!df) {
        fprintf(stderr, "Could not find .desktop file: %s\n", desktop_name);
        return 1;
    }

    char line[MAX_LINE], exec_cmd[256] = "", full_exec_line[MAX_LINE] = "";
    while (fgets(line, sizeof(line), df)) {
        if (strncasecmp(line, "Exec=", 5) == 0) {
            snprintf(full_exec_line, sizeof(full_exec_line), "%s", line + 5);
            full_exec_line[strcspn(full_exec_line, "\r\n")] = '\0';
            extract_exec(line, exec_cmd, sizeof(exec_cmd));
            break;
        }
    }
    fclose(df);
    if (exec_cmd[0] == '\0') {
        fprintf(stderr, "No Exec line found in %s\n", desktop_path);
        return 1;
    }

    // Compare by program name, whether Exec= gives a bare name or a full path
    const char *exe = strrchr(exec_cmd, '/');
    exe = exe ? exe + 1 : exec_cmd;

    char *dir = strdup(filepath);
    if (!dir) {
        perror("strdup");
        return 1;
    }
    char *dirpath = dirname(dir);
    int rc;

    // Flatpak-specific logic
    if (strcmp(exe, "flatpak") == 0) {
        char *run_ptr = strstr(full_exec_line, "run ");
        if (run_ptr) {
            char app_id[256];
            if (sscanf(run_ptr + 4, "%255s", app_id) == 1 && is_flatpak_installed(app_id)) {
                if (strstr(app_id, "dolphin")) {
                    char *cmd[] = { "flatpak", "run", app_id, "--select", filepath, NULL };
                    rc = spawn_detached(cmd);
                } else if (strstr(app_id, "nemo")) {
                    char *cmd[] = { "flatpak", "run", app_id, filepath, NULL };
                    rc = spawn_detached(cmd);
                } else {
                    char *cmd[] = { "flatpak", "run", app_id, dirpath, NULL };
                    rc = spawn_detached(cmd);
                }
                free(dir);
                return rc == 0 ? 0 : 1;
            }
        }
    }

    // Native file managers
    if (strcmp(exe, "dolphin") == 0) {
        char *cmd[] = { exec_cmd, "--select", filepath, NULL };
        rc = spawn_detached(cmd);
    } else if (strcmp(exe, "nemo") == 0) {
        char *cmd[] = { exec_cmd, filepath, NULL };
        rc = spawn_detached(cmd);
    } else if (strcmp(exe, "nautilus") == 0 ||
               strcmp(exe, "thunar") == 0 ||
               strcmp(exe, "pcmanfm") == 0) {
        char *cmd[] = { exec_cmd, dirpath, NULL };
        rc = spawn_detached(cmd);
    } else {
        char *cmd[] = { "xdg-open", dirpath, NULL };
        rc = spawn_detached(cmd);
    }

    free(dir);
    return rc == 0 ? 0 : 1;
}

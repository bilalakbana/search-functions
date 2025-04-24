#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libgen.h>
#include <unistd.h>
#include <limits.h>

#define MAX_CMD 4096
#define MAX_LINE 1024

// Execute a system command while safely ignoring its return value
void run_silently(const char *cmd) {
    (void)!system(cmd);
}

// Safely quote file paths for shell usage
void shell_quote(const char *in, char *out, size_t max) {
    size_t j = 0;
    out[j++] = '"';
    for (size_t i = 0; in[i] && j + 2 < max; ++i) {
        if (in[i] == '"') out[j++] = '\\';
        out[j++] = in[i];
    }
    out[j++] = '"';
    out[j] = '\0';
}

// Extract executable name from Exec line
void extract_exec(const char *line, char *out) {
    const char *p = strchr(line, '=');
    if (!p) {
        out[0] = '\0';
        return;
    }
    p++;
    while (*p == ' ') p++;
    sscanf(p, "%s", out);
}

// Flatpak app installed check
int is_flatpak_installed(const char *app_id) {
    char command[256];
    snprintf(command, sizeof(command),
             "flatpak list --app --columns=application | grep -Fxq \"%s\"", app_id);
    return system(command) == 0;
}

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s /path/to/file\n", argv[0]);
        return 1;
    }

    const char *filepath = argv[1];
    char desktop_name[256];
    FILE *fp = popen("xdg-mime query default inode/directory", "r");
    if (!fp || !fgets(desktop_name, sizeof(desktop_name), fp)) {
        if (fp) pclose(fp);
        fprintf(stderr, "Failed to get default file manager\n");
        return 1;
    }
    pclose(fp);
    desktop_name[strcspn(desktop_name, "\n")] = 0;

    // Open .desktop file
    char desktop_path[PATH_MAX];
    snprintf(desktop_path, sizeof(desktop_path), "/usr/share/applications/%s", desktop_name);
    FILE *df = fopen(desktop_path, "r");
    if (!df) {
        const char *home = getenv("HOME");
        if (!home) {
            fprintf(stderr, "Could not get $HOME\n");
            return 1;
        }
        snprintf(desktop_path, sizeof(desktop_path),
                 "%s/.local/share/applications/%s", home, desktop_name);
        df = fopen(desktop_path, "r");
        if (!df) {
            fprintf(stderr, "Could not find .desktop file: %s\n", desktop_name);
            return 1;
        }
    }

    char line[MAX_LINE], exec_cmd[256] = "", full_exec_line[512] = "";
    while (fgets(line, sizeof(line), df)) {
        if (strncasecmp(line, "Exec=", 5) == 0) {
            strncpy(full_exec_line, line + 5, sizeof(full_exec_line) - 1);
            full_exec_line[sizeof(full_exec_line) - 1] = '\0'; // Ensure null-termination
            extract_exec(line, exec_cmd);
            break;
        }
    }
    fclose(df);
    if (exec_cmd[0] == '\0') {
        fprintf(stderr, "No Exec line found in %s\n", desktop_path);
        return 1;
    }

    // Quoted paths
    char qfilepath[PATH_MAX], qdir[PATH_MAX];
    shell_quote(filepath, qfilepath, sizeof(qfilepath));

    char *dir = strdup(filepath);
    if (!dir) {
        perror("strdup");
        return 1;
    }
    shell_quote(dirname(dir), qdir, sizeof(qdir));

    char command[MAX_CMD];
    int written = 0;

    // Flatpak-specific logic
    if (strcmp(exec_cmd, "flatpak") == 0 && strstr(full_exec_line, "run")) {
        char *run_ptr = strstr(full_exec_line, "run ");
        if (run_ptr) {
            char app_id[256];
            sscanf(run_ptr + 4, "%255s", app_id);
            if (is_flatpak_installed(app_id)) {
                if (strstr(app_id, "dolphin")) {
                    written = snprintf(command, sizeof(command),
                                       "nohup flatpak run %s --select %s &>/dev/null &", app_id, qfilepath);
                } else if (strstr(app_id, "nemo")) {
                    written = snprintf(command, sizeof(command),
                                       "nohup flatpak run %s %s &>/dev/null &", app_id, qfilepath);
                } else {
                    written = snprintf(command, sizeof(command),
                                       "nohup flatpak run %s %s &>/dev/null &", app_id, qdir);
                }
                if (written >= (int)sizeof(command)) {
                    fprintf(stderr, "Warning: command truncated.\n");
                }
                free(dir);
                run_silently(command);
                return 0;
            }
        }
    }

    // Native FMs
    if (strcmp(exec_cmd, "dolphin") == 0) {
        written = snprintf(command, sizeof(command),
                           "nohup dolphin --select %s &>/dev/null &", qfilepath);
    } else if (strcmp(exec_cmd, "nemo") == 0) {
        written = snprintf(command, sizeof(command),
                           "nohup nemo %s &>/dev/null &", qfilepath);
    } else if (strcmp(exec_cmd, "nautilus") == 0 ||
        strcmp(exec_cmd, "thunar") == 0 ||
        strcmp(exec_cmd, "pcmanfm") == 0) {
        written = snprintf(command, sizeof(command),
                           "nohup %s %s &>/dev/null &", exec_cmd, qdir);
        } else {
            written = snprintf(command, sizeof(command),
                               "nohup xdg-open %s &>/dev/null &", qdir);
        }

        if (written >= (int)sizeof(command)) {
            fprintf(stderr, "Warning: command truncated.\n");
        }

        free(dir);
        run_silently(command);
        return 0;
}

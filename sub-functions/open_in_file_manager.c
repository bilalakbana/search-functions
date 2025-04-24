#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <libgen.h>
#include <unistd.h>

#define MAX_CMD 1024
#define MAX_LINE 1024

// Return the first word (executable) from an Exec line
void extract_exec(char *line, char *out) {
    char *p = strchr(line, '=');
    if (!p) {
        out[0] = '\0';
        return;
    }
    p++; // move past '='
    while (*p == ' ') p++; // skip spaces
    sscanf(p, "%s", out); // read the first word (executable)
}

// Check if a given Flatpak app ID is installed
int is_flatpak_installed(const char *app_id) {
    char command[256];
    snprintf(command, sizeof(command), "flatpak list --app --columns=application | grep -Fxq \"%s\"", app_id);
    int result = system(command);
    return result == 0;
}

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s /path/to/file\n", argv[0]);
        return 1;
    }

    char *filepath = argv[1];

    FILE *fp = popen("xdg-mime query default inode/directory", "r");
    if (!fp) {
        perror("popen failed");
        return 1;
    }

    char desktop_name[256];
    if (!fgets(desktop_name, sizeof(desktop_name), fp)) {
        pclose(fp);
        fprintf(stderr, "Failed to get default file manager\n");
        return 1;
    }
    pclose(fp);
    desktop_name[strcspn(desktop_name, "\n")] = 0; // trim newline

    // Check both global and user application dirs
    char desktop_path[512];
    snprintf(desktop_path, sizeof(desktop_path), "/usr/share/applications/%s", desktop_name);
    FILE *df = fopen(desktop_path, "r");

    if (!df) {
        snprintf(desktop_path, sizeof(desktop_path), "%s/.local/share/applications/%s", getenv("HOME"), desktop_name);
        df = fopen(desktop_path, "r");
        if (!df) {
            fprintf(stderr, "Could not find .desktop file: %s\n", desktop_name);
            return 1;
        }
    }

    char line[MAX_LINE];
    char exec_cmd[256] = "";
    char full_exec_line[512] = "";

    while (fgets(line, sizeof(line), df)) {
        if (strncasecmp(line, "Exec=", 5) == 0) {
            strncpy(full_exec_line, line + 5, sizeof(full_exec_line) - 1);
            extract_exec(line, exec_cmd);
            break;
        }
    }
    fclose(df);

    if (exec_cmd[0] == '\0') {
        fprintf(stderr, "No Exec line found\n");
        return 1;
    }

    // Handle flatpak case
    char command[MAX_CMD];
    char *dir = strdup(filepath);

    if (strcmp(exec_cmd, "flatpak") == 0 && strstr(full_exec_line, "run")) {
        // Try to extract the Flatpak app ID (2nd token in the Exec line)
        char *run_ptr = strstr(full_exec_line, "run ");
        if (run_ptr) {
            char app_id[256];
            sscanf(run_ptr + 4, "%255s", app_id); // read after "run "

            if (is_flatpak_installed(app_id)) {
                if (strstr(app_id, "dolphin")) {
                    snprintf(command, sizeof(command), "nohup flatpak run %s --select \"%s\" &>/dev/null &", app_id, filepath);
                } else if (strstr(app_id, "nemo")) {
                    snprintf(command, sizeof(command), "nohup flatpak run %s \"%s\" &>/dev/null &", app_id, filepath);
                } else {
                    snprintf(command, sizeof(command), "nohup flatpak run %s \"%s\" &>/dev/null &", app_id, dirname(dir));
                }
                free(dir);
                system(command);
                return 0;
            }
        }
    }

    // Handle regular installed file managers
    if (strcmp(exec_cmd, "dolphin") == 0) {
        snprintf(command, sizeof(command), "nohup dolphin --select \"%s\" &>/dev/null &", filepath);
    } else if (strcmp(exec_cmd, "nemo") == 0) {
        snprintf(command, sizeof(command), "nohup nemo \"%s\" &>/dev/null &", filepath);
    } else if (strcmp(exec_cmd, "nautilus") == 0 || strcmp(exec_cmd, "thunar") == 0 || strcmp(exec_cmd, "pcmanfm") == 0) {
        snprintf(command, sizeof(command), "nohup %s \"%s\" &>/dev/null &", exec_cmd, dirname(dir));
    } else {
        snprintf(command, sizeof(command), "nohup xdg-open \"%s\" &>/dev/null &", dirname(dir));
    }

    free(dir);
    system(command);
    return 0;
}

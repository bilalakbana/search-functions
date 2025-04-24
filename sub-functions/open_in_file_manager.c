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

int main(int argc, char *argv[]) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s /path/to/file\n", argv[0]);
        return 1;
    }

    char *filepath = argv[1];
    char *desktop_file = NULL;
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

    // remove newline
    desktop_name[strcspn(desktop_name, "\n")] = 0;

    char path[512];
    snprintf(path, sizeof(path), "/usr/share/applications/%s", desktop_name);
    FILE *df = fopen(path, "r");

    if (!df) {
        snprintf(path, sizeof(path), "%s/.local/share/applications/%s", getenv("HOME"), desktop_name);
        df = fopen(path, "r");
        if (!df) {
            fprintf(stderr, "Could not find .desktop file\n");
            return 1;
        }
    }

    char line[MAX_LINE], exec_cmd[256] = "";
    while (fgets(line, sizeof(line), df)) {
        if (strncasecmp(line, "Exec=", 5) == 0) {
            extract_exec(line, exec_cmd);
            break;
        }
    }
    fclose(df);

    if (exec_cmd[0] == '\0') {
        fprintf(stderr, "No Exec line found\n");
        return 1;
    }

    char command[MAX_CMD];
    if (strcmp(exec_cmd, "dolphin") == 0) {
        snprintf(command, sizeof(command), "nohup dolphin --select \"%s\" &>/dev/null &", filepath);
    } else if (strcmp(exec_cmd, "nemo") == 0) {
        snprintf(command, sizeof(command), "nohup nemo \"%s\" &>/dev/null &", filepath);
    } else if (strcmp(exec_cmd, "nautilus") == 0 || strcmp(exec_cmd, "thunar") == 0 || strcmp(exec_cmd, "pcmanfm") == 0) {
        char *dir = strdup(filepath);
        snprintf(command, sizeof(command), "nohup %s \"%s\" &>/dev/null &", exec_cmd, dirname(dir));
        free(dir);
    } else {
        char *dir = strdup(filepath);
        snprintf(command, sizeof(command), "nohup xdg-open \"%s\" &>/dev/null &", dirname(dir));
        free(dir);
    }

    system(command);
    return 0;
}

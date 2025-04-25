/* is_text_file.c
 *   compile:  gcc -Wall -Wextra -O2 is_text_file.c -o is_text_file
 *   usage  :  ./is_text_file  pathToFile
 *   exit   :  0 on success, 1 on I/O error or wrong arguments               */

#include <stdio.h>
#include <ctype.h>
#include <stdbool.h>

#define CHUNK 8192          /* how many bytes we sample          */
#define MIN_PRINTABLE 0.95  /* 95 % printable threshold          */

bool is_text_file(const char *path)
{
    FILE *fp = fopen(path, "rb");
    if (!fp)                       /* unreadable ⇒ treat as binary */
        return false;

    unsigned char buf[CHUNK];
    size_t nread = fread(buf, 1, CHUNK, fp);
    fclose(fp);

    /* ---------- “assume binary” baseline ---------- */
    bool looks_text = false;       /* <- default is *binary* */

    if (nread) {
        size_t printable = 0;

        for (size_t i = 0; i < nread; ++i) {
            if (buf[i] == '\0')    /* any NUL ⇒ keep binary verdict */
                return false;

            if (isprint(buf[i]) || isspace(buf[i]))
                ++printable;
        }

        /* promote to text only if strong evidence */
        double ratio = (double) printable / nread;
        looks_text = (ratio >= MIN_PRINTABLE);
    }

    return looks_text;             /* false ⇒ binary, true ⇒ text  */
}

/* ------------------ small CLI wrapper ------------------ */
int main(int argc, char *argv[])
{
    if (argc != 2) {
        fprintf(stderr, "Usage: %s pathToFile\n", argv[0]);
        return 1;
    }

    int r = is_text_file(argv[1]);
    if (r == -1) {
        perror(argv[1]);                /* fopen failed */
        return 1;
    }

    puts(r ? "true" : "false");
    return 0;
}

/* is_text_file.c
 *   compile:  gcc -Wall -Wextra -O2 is_text_file.c -o is_text_file
 *   usage  :  ./is_text_file  pathToFile
 *   output :  "true" or "false" on stdout
 *   exit   :  0 on success, 1 on I/O error or wrong arguments (nothing on stdout)  */

#include <stdio.h>
#include <ctype.h>

#define CHUNK 8192          /* how many bytes we sample          */
#define MIN_PRINTABLE 0.95  /* 95 % printable threshold          */

/* 1 = text, 0 = binary, -1 = cannot open */
static int is_text_file(const char *path)
{
    FILE *fp = fopen(path, "rb");
    if (!fp)
        return -1;

    unsigned char buf[CHUNK];
    size_t nread = fread(buf, 1, CHUNK, fp);
    fclose(fp);

    if (nread == 0)                /* empty file: no evidence of text */
        return 0;

    size_t printable = 0;
    for (size_t i = 0; i < nread; ++i) {
        if (buf[i] == '\0')        /* any NUL => binary */
            return 0;

        /* bytes >= 0x80 are UTF-8 (or Latin-1) text, not control characters */
        if (buf[i] >= 0x80 || isprint(buf[i]) || isspace(buf[i]))
            ++printable;
    }

    /* promote to text only if strong evidence */
    return (double) printable / nread >= MIN_PRINTABLE;
}

/* ------------------ small CLI wrapper ------------------ */
int main(int argc, char *argv[])
{
    if (argc != 2) {
        fprintf(stderr, "Usage: %s pathToFile\n", argv[0]);
        return 1;
    }

    int r = is_text_file(argv[1]);
    if (r < 0) {
        perror(argv[1]);                /* fopen failed */
        return 1;
    }

    puts(r ? "true" : "false");
    return 0;
}

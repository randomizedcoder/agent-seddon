/* FAIL fixture (fail-cols): row output ignores the SPEC indices (positional fields)
 * Reference solution for the csv-slice objective — the PASS fixture (R13).
 * C99 + libc only; every graded contract from CONSTRAINTS.md implemented.
 * _POSIX_C_SOURCE: getline/strtok_r are POSIX-2008 — without the macro,
 * -std=c99 hides their declarations (an ERROR on gcc >= 14). */
#define _POSIX_C_SOURCE 200809L
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/types.h>

#define MAX_COLS 64

static void die_bad_args(void) {
    fprintf(stderr, "csv-slice: bad arguments\n");
    exit(64);
}

/* One selected output column: header name + 0-based input index. */
struct sel {
    char *name;
    long idx;
};

/* Parse "name:idx,name:idx,..." into sels; returns the count or -1. */
static int parse_spec(char *spec, struct sel *sels) {
    int n = 0;
    char *save = NULL;
    for (char *tok = strtok_r(spec, ",", &save); tok != NULL;
         tok = strtok_r(NULL, ",", &save)) {
        if (n >= MAX_COLS) {
            return -1;
        }
        char *colon = strchr(tok, ':');
        if (colon == NULL || colon == tok || colon[1] == '\0') {
            return -1;
        }
        *colon = '\0';
        char *end = NULL;
        long idx = strtol(colon + 1, &end, 10);
        if (*end != '\0' || idx < 0) {
            return -1;
        }
        sels[n].name = tok;
        sels[n].idx = idx;
        n++;
    }
    return n > 0 ? n : -1;
}

int main(int argc, char **argv) {
    const char *input = NULL;
    char *cols = NULL;
    char *where = NULL;

    for (int i = 1; i < argc; i++) {
        if (strcmp(argv[i], "--version") == 0) {
            printf("csv-slice 2.4.0-arena\n");
            return 0;
        } else if (strcmp(argv[i], "--input") == 0 && i + 1 < argc) {
            input = argv[++i];
        } else if (strcmp(argv[i], "--cols") == 0 && i + 1 < argc) {
            cols = argv[++i];
        } else if (strcmp(argv[i], "--where") == 0 && i + 1 < argc) {
            where = argv[++i];
        } else {
            die_bad_args();
        }
    }
    if (input == NULL || cols == NULL) {
        die_bad_args();
    }

    struct sel sels[MAX_COLS];
    int nsel = parse_spec(cols, sels);
    if (nsel < 0) {
        die_bad_args();
    }

    long where_idx = -1;
    const char *where_val = NULL;
    if (where != NULL) {
        char *eq = strchr(where, '=');
        if (eq == NULL || eq == where) {
            die_bad_args();
        }
        *eq = '\0';
        char *end = NULL;
        where_idx = strtol(where, &end, 10);
        if (*end != '\0' || where_idx < 0) {
            die_bad_args();
        }
        where_val = eq + 1;
    }

    /* The widest input index any part of the selection needs. */
    long need = where_idx;
    for (int i = 0; i < nsel; i++) {
        if (sels[i].idx > need) {
            need = sels[i].idx;
        }
    }

    FILE *f = fopen(input, "r");
    if (f == NULL) {
        fprintf(stderr, "csv-slice: cannot open %s\n", input);
        return 66;
    }

    for (int i = 0; i < nsel; i++) {
        fputs(sels[i].name, stdout);
        putchar(i + 1 < nsel ? ',' : '\n');
    }

    char *line = NULL;
    size_t cap = 0;
    long row = 0;
    long skipped = 0;
    ssize_t got;
    while ((got = getline(&line, &cap, f)) != -1) {
        row++;
        if (got > 0 && line[got - 1] == '\n') {
            line[--got] = '\0';
        }
        /* CONSTRAINTS.md rule 3: an embedded NUL makes strlen come up short. */
        if ((ssize_t)strlen(line) != got) {
            fprintf(stderr, "csv-slice: binary garbage at row %ld\n", row);
            free(line);
            fclose(f);
            return 65;
        }
        /* Split in place on ',' only (the documented dialect). */
        char *fields[1024];
        long nfields = 0;
        char *p = line;
        while (nfields < 1024) {
            fields[nfields++] = p;
            char *comma = strchr(p, ',');
            if (comma == NULL) {
                break;
            }
            *comma = '\0';
            p = comma + 1;
        }
        if (need >= nfields) {
            skipped++; /* CONSTRAINTS.md rule 4: tolerated, counted. */
            continue;
        }
        if (where_val != NULL && strcmp(fields[where_idx], where_val) != 0) {
            continue;
        }
        for (int i = 0; i < nsel; i++) {
            fputs(fields[i], stdout);
            putchar(i + 1 < nsel ? ',' : '\n');
        }
    }
    free(line);
    fclose(f);
    if (skipped > 0) {
        fprintf(stderr, "skipped %ld short rows\n", skipped);
    }
    return 0;
}

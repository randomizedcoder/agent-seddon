/* gen — deterministic headerless-CSV generator for the perf floor.
 * Usage: ./gen N OUT   (writes N rows of 4 columns to OUT)
 * Shipped with the seed; graded runs compile and invoke it — DO NOT MODIFY. */
#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: gen N OUT\n");
        return 2;
    }
    long n = strtol(argv[1], NULL, 10);
    if (n <= 0) {
        fprintf(stderr, "gen: bad N\n");
        return 2;
    }
    FILE *f = fopen(argv[2], "w");
    if (!f) {
        fprintf(stderr, "gen: cannot open %s\n", argv[2]);
        return 2;
    }
    for (long i = 0; i < n; i++) {
        fprintf(f, "%ld,val%ld,tag%ld,city%ld\n", i, i % 97, i % 7, i % 13);
    }
    fclose(f);
    return 0;
}

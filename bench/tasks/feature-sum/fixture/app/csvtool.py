import csv, sys

def main(argv):
    if len(argv) >= 3 and argv[1] == "cols":
        with open(argv[2]) as f:
            print(",".join(csv.reader(f).__next__()))
        return
    print("usage: cols <file>", file=sys.stderr)
    sys.exit(2)

if __name__ == "__main__":
    main(sys.argv)

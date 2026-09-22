import sys
from runner import run


def main(argv):
    verbose = "--verbose" in argv
    args = [a for a in argv if a != "--verbose"]
    print(run(args[0]))


if __name__ == "__main__":
    main(sys.argv[1:])

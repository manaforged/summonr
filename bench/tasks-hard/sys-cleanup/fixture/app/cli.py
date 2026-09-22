import sys
from exporter import export, export_legacy

HELP = "usage: cli.py [--legacy] export NAME"


def main(argv):
    if "--help" in argv or not argv:
        print(HELP)
        return
    legacy = "--legacy" in argv
    args = [a for a in argv if a != "--legacy"]
    if args[0] == "export":
        if legacy:
            print(export_legacy(args[1]))
        else:
            print(export(args[1]))


if __name__ == "__main__":
    main(sys.argv[1:])

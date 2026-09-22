import sys
from report import parse_rows, render_table

if __name__ == "__main__":
    print(render_table(parse_rows(sys.stdin.read())))

import sys

def main(argv):
    nums = [float(a) for a in argv[1:]]
    print(f"min {min(nums)}")
    print(f"mean {sum(nums)/len(nums):.2f}")
    print(f"max {max(nums)}")

if __name__ == "__main__":
    main(sys.argv)

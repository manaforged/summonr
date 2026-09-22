import os

def config_dir():
    return os.path.expanduser("~/.myapp")

if __name__ == "__main__":
    print(config_dir())

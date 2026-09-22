class Engine:
    def __init__(self, verbose=False):
        self.verbose = verbose

    def start(self, job):
        if self.verbose:
            print(f"engine: starting {job}")

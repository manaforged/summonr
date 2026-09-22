PLUGINS = {}


class Registry:
    def __init__(self):
        self.plugins = PLUGINS

    def register(self, name, fn):
        self.plugins[name] = fn

    def run(self, name):
        return self.plugins[name]()

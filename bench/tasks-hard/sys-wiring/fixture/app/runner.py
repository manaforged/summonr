from engine import Engine


def run(job):
    eng = Engine()
    eng.start(job)
    return f"done: {job}"

CONFIG = {"retries": 3, "timeout": 30}


def describe():
    return f"retries={CONFIG['retries']} timeout={CONFIG['timeout']}"


def will_retry(attempt):
    return attempt < CONFIG["retries"]

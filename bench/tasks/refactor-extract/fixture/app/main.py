def slug(text):
    return "-".join(text.lower().split())

def title_of(text):
    return text.title()

def run(name):
    return f"{title_of(name)} [{slug(name)}]"

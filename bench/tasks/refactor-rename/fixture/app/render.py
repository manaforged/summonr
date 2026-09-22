from net import fetch_url

def render(url):
    return fetch_url(url).upper()

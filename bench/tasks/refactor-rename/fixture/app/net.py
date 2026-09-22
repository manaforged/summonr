def fetch_url(url):
    return f"<body of {url}>"

def fetch_all(urls):
    return [fetch_url(u) for u in urls]

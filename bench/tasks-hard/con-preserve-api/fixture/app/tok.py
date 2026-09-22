def tokenize(text):
    tokens = []
    cur = ""
    for ch in text:
        if ch == " ":
            tokens.append(cur)
            cur = ""
        else:
            cur += ch
    tokens.append(cur)
    return tokens


def detokenize(tokens, sep=" "):
    return sep.join(tokens)

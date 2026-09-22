def page(items, page_num, per_page):
    start = page_num * per_page
    return items[start:start + per_page]

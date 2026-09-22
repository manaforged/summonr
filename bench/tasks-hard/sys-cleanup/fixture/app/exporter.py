import json


def export(name):
    return json.dumps({"name": name})


def export_legacy(name):
    # deprecated text format kept for old scripts
    return "NAME=" + name

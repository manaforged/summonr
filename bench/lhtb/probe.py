import os
import shlex

from harbor.agents.base import BaseAgent

CHECK = r'''
import os, urllib.request
key = os.environ["DEEPSEEK_API_KEY"]
cases = [
    ("https://example.com", {}),
    ("http://example.com", {}),
    ("http://1.1.1.1", {}),
    ("http://172.17.0.1:22", {}),
    ("http://172.17.0.1:8094/probe0000000000/v1/models", {"Authorization": "Bearer " + key}),
    ("http://172.17.0.1:22/", {"Host": "bench-relay:8094"}),
    ("http://93.184.215.14/", {"Host": "bench-relay:8094"}),
    ("http://bench-relay:8094/probe0000000000/v1/models", {"Authorization": "Bearer " + key}),
]
for url, headers in cases:
    try:
        status = urllib.request.urlopen(urllib.request.Request(url, headers=headers), timeout=8).status
    except Exception as exc:
        status = type(exc).__name__ + ": " + str(exc)[:80]
    print(url, headers.get("Host", ""), status)
'''


class Probe(BaseAgent):
    @staticmethod
    def name():
        return "probe"

    def version(self):
        return "1"

    async def setup(self, environment):
        await environment.exec("mkdir -p /logs/agent")

    async def run(self, instruction, environment, context):
        await environment.exec(
            f"python3 -c {shlex.quote(CHECK)} > /logs/agent/probe.txt 2>&1",
            env={"DEEPSEEK_API_KEY": os.environ["DEEPSEEK_API_KEY"]},
        )

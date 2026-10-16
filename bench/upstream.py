import http.client
import urllib.request

READ_SECONDS = 900


class Connecting:
    connect_seconds = 30.0

    def connect(self):
        self.timeout = self.connect_seconds
        super().connect()
        self.sock.settimeout(READ_SECONDS)


class PlainConnection(Connecting, http.client.HTTPConnection):
    pass


class SecureConnection(Connecting, http.client.HTTPSConnection):
    pass


class Plain(urllib.request.HTTPHandler):
    def http_open(self, req):
        return self.do_open(PlainConnection, req)


class Secure(urllib.request.HTTPSHandler):
    def https_open(self, req):
        return self.do_open(SecureConnection, req, context=self._context)


def opener(connect_seconds):
    Connecting.connect_seconds = connect_seconds
    return urllib.request.build_opener(Plain, Secure)

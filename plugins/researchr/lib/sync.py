import os
import shutil
import subprocess
import tempfile

from audit import import_records


def sync(store, session, captures):
    executable = os.environ.get("SUMMONR_BIN") or shutil.which("summonr") or "summonr"
    with tempfile.TemporaryFile(dir=store.path.parent) as output:
        with tempfile.TemporaryFile(dir=store.path.parent) as errors:
            done = subprocess.run([executable, "session", "export", session, "--json"],
                                  stdin=subprocess.DEVNULL, stdout=output, stderr=errors,
                                  timeout=store.limits.sync_secs, check=False)
            errors.seek(0)
            diagnostic = errors.read(store.limits.text_bytes).decode("utf-8", "backslashreplace")
        if done.returncode:
            raise ValueError(f"native audit export exited {done.returncode}: {diagnostic or 'no diagnostic'}")
        output.seek(0)
        try:
            result = import_records(store, output, False)
        except ValueError as error:
            raise ValueError(f"native audit import failed: {error}; verify the installed binary at {executable}") from error
    result["exporter"] = executable
    if len(result["sessions"]) != 1:
        raise ValueError("native export did not identify exactly one session")
    native = result["sessions"][0]
    result["capture_streams"] = {"files": 0, "inserted": 0, "records": 0, "partial": 0}
    result["provider_coverage"] = "unavailable: capture not requested"
    if captures is not None:
        folder = captures.resolve(strict=True) / native
        if not folder.is_dir():
            result["capture_warning"] = "no provider capture for this session; raw provider fields are unavailable"
            result["provider_coverage"] = "unavailable: no raw capture for this session"
            return result
        result["provider_coverage"] = "captured prefixes; exchange completion is not inferred"
        for file in sorted(folder.glob("*.jsonl")):
            with file.open("rb") as source:
                imported = import_records(store, source, True, native)
            counts = result["capture_streams"]
            counts["files"] += 1
            counts["inserted"] += imported["inserted"]
            counts["records"] += imported["records"]
            counts["partial"] += not imported["complete_export"]
    return result

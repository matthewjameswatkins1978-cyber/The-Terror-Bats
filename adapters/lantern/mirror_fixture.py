"""Exercise the actual Lantern exporter with bounded loopback read-only fixtures."""
import argparse, hashlib, http.server, importlib.util, json, subprocess, tempfile, threading
from pathlib import Path


def run(target, mode):
    spec = importlib.util.spec_from_file_location("lantern_export", target / "scripts/lantern_git_export.py")
    module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module)
    record = {"id":"ed7e2de6-2d4d-464a-903d-f41df1b990f3", "kind":"reference", "content":"fixture line one\nfixture line two", "originator_actor_id":"fixture", "holder_actor_id":"fixture"}
    class Handler(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            self.rfile.read(int(self.headers.get("Content-Length", "0"))); self.do_GET()
        def do_GET(self):
            values = {"/api/v1/version":{"service":"fixture", "project":"lantern", "version":"disposable"},
                      "/api/v1/memory-items/search":{"memory_items":[record]}, "/api/v1/claims?unmapped=false":{"claims":[]},
                      "/api/v1/beliefs?include_stale=true":{"beliefs":[]}, "/api/v1/memories/recall":{"memories":[]}, "/api/v1/projects":{"projects":[]}}
            data=json.dumps(values[self.path]).encode(); self.send_response(200); self.end_headers(); self.wfile.write(data)
        def log_message(self, *args): pass
    server=http.server.ThreadingHTTPServer(("127.0.0.1",0),Handler)
    thread=threading.Thread(target=server.serve_forever,daemon=True); thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="tb-lantern-mirror-") as directory:
            root=Path(directory)
            args=argparse.Namespace(service_url=f"http://127.0.0.1:{server.server_port}",output=str(root),require_marker=None,require_uuid=None)
            manifest=module.export(args); mirror=root/"mirror"
            before={str(p.relative_to(mirror)):hashlib.sha256(p.read_bytes()).hexdigest() for p in mirror.rglob("*") if p.is_file()}
            if mode == "atomic":
                args.require_marker="absent exact marker";args.require_uuid=record["id"]
                rejected=False
                try: module.export(args)
                except RuntimeError as error: rejected="required marker" in str(error)
                after={str(p.relative_to(mirror)):hashlib.sha256(p.read_bytes()).hexdigest() for p in mirror.rglob("*") if p.is_file()}
                return {"scenario":mode,"validation_rejected":rejected,"before":before,"after":after,"violation":not rejected or before!=after}
            def git(*args):
                return subprocess.check_output(["git","-C",str(root),*args],stderr=subprocess.PIPE)
            git("init");git("config","user.name","fixture");git("config","user.email","fixture@example.invalid");git("config","commit.gpgsign","false")
            git("config","core.autocrlf","true" if mode=="crlf" else "false");git("add","mirror");git("commit","-m","disposable export")
            if mode=="crlf":
                for p in mirror.rglob("*"):
                    if p.is_file(): p.unlink()
                git("checkout","HEAD","--","mirror")
            rows=[]
            for sidecar in mirror.rglob("*.integrity.json"):
                entry=json.loads(sidecar.read_text());data=(mirror/entry["path"]).read_bytes()
                blob=git("rev-parse",f"HEAD:mirror/{entry['path']}").decode().strip()
                actual=hashlib.sha256(data).hexdigest();git_bytes=git("show",f"HEAD:mirror/{entry['path']}")
                rows.append({"path":entry["path"],"sidecar_sha256":entry["sha256"],"file_sha256":actual,"sidecar_git_blob_sha":entry["git_blob_sha"],"git_blob_sha":blob,"git_bytes_sha256":hashlib.sha256(git_bytes).hexdigest(),"file_bytes":len(data),"sidecar_bytes":entry["bytes"],"contains_crlf":b"\r\n" in data})
            status=json.loads((mirror/"status.json").read_text());manifest_bytes=(mirror/"manifest.json").read_bytes()
            digest="sha256:"+hashlib.sha256(manifest_bytes).hexdigest()
            index_rows=[];index_shards=[];headers_agree=True
            for state,paths in manifest["index_paths"].items():
                for path in paths:
                    shard=[json.loads(line) for line in (mirror/path).read_text().splitlines() if line]
                    if not shard or "_index" not in shard[0]:raise RuntimeError("unsupported/missing actual export index header")
                    header=shard[0]["_index"]
                    headers_agree=headers_agree and header.get("format")=="lantern-git-jsonl-v1" and header.get("state")==state and header.get("record_count")==len(shard)-1
                    index_shards.append({"path":path,"header":header,"rows":shard[1:]});index_rows.extend(shard[1:])
            sidecars={r["path"]:r for r in rows}
            index_agrees=headers_agree and len(index_rows)==len(rows) and all(r["path"] in sidecars and r["sha256"]==sidecars[r["path"]]["sidecar_sha256"] and r["git_blob_sha"]==sidecars[r["path"]]["sidecar_git_blob_sha"] and r["bytes"]==sidecars[r["path"]]["sidecar_bytes"] for r in index_rows)
            counts_agree=manifest["record_count"]==len(rows)==status["record_count"] and manifest["counts_by_type"]==status["counts_by_type"]=={"memory-item":1,"memory":0,"claim":0,"belief":0} and manifest["active_count"]==status["active_count"]==1 and manifest["archived_count"]==status["archived_count"]==0
            bad=not index_agrees or not counts_agree or any(r["sidecar_sha256"]!=r["file_sha256"] or r["sidecar_git_blob_sha"]!=r["git_blob_sha"] or r["file_bytes"]!=r["sidecar_bytes"] for r in rows)
            return {"scenario":mode,"rows":rows,"manifest":manifest,"manifest_byte_digest":digest,"status":status,"index_rows":index_rows,"index_shards":index_shards,"index_agrees":index_agrees,"counts_agree":counts_agree,"violation":bad or status["manifest_digest"]!=digest}
    finally:
        server.shutdown();server.server_close();thread.join()
#!/usr/bin/env python3
"""Real Linux process-isolation test: authority UID nobody, MCP client caller UID.

Requires passwordless sudo and a disposable temp workspace. Nothing installed.
"""
import json
import hmac
import hashlib
import os
from pathlib import Path
import select
import shutil
import socket
import subprocess
import sys
import tempfile
import time

authority_bin, mcp_bin = map(str, map(Path, sys.argv[1:3]))

def run(*args, **kwargs):
    return subprocess.run(args, check=True, capture_output=True, text=True, **kwargs)

def receive_line(stream, timeout=8):
    ready, _, _ = select.select([stream], [], [], timeout)
    assert ready, "MCP server response timed out"
    line = stream.readline()
    assert line, "MCP server closed stdout unexpectedly"
    return json.loads(line)

class Mcp:
    def __init__(self, socket_path, key_path):
        env={**os.environ,"TAIYAKI_SOCKET_PATH":str(socket_path),"TAIYAKI_SECRET_FILE":str(key_path)}
        self.child = subprocess.Popen([mcp_bin], stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,env=env,text=True,bufsize=1)
    def call(self, number, method, params):
        self.child.stdin.write(json.dumps({"jsonrpc":"2.0","id":number,"method":method,"params":params})+"\n")
        self.child.stdin.flush()
        result=receive_line(self.child.stdout)
        assert result.get("id")==number,(number,result)
        return result
    def initialize(self):
        data=self.call(1,"initialize",{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"taiyaki-e2e","version":"1"}})
        assert data["result"]["serverInfo"]["name"]=="taiyaki",data
        self.child.stdin.write(json.dumps({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})+"\n")
        self.child.stdin.flush()
    def tool(self, number, name, arguments=None):
        return self.call(number,"tools/call",{"name":name,"arguments":arguments or {}})["result"]
    def close(self):
        if self.child.poll() is None:
            self.child.terminate()
            try:self.child.communicate(timeout=3)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.communicate()

def main():
    assert os.getuid()!=65534,"tests require separate caller from nobody"
    with tempfile.TemporaryDirectory(prefix="taiyaki-os-auth-") as temp:
        root=Path(temp)
        root.chmod(0o711)
        for name,mode in (("socket",0o711),("database",0o700)):
            folder=root/name
            folder.mkdir(mode=mode)
            run("sudo","-n","chown","65534:65534",str(folder))
        secret=os.urandom(32).hex()+"\n"
        key_file=root/"client.key"
        key_file.write_text(secret)
        key_file.chmod(0o600)
        bogus=root/"bad.key"
        bogus.write_text(os.urandom(32).hex()+"\n")
        bogus.chmod(0o600)
        authority_secret=root/"database"/"authority.key"
        run("sudo","-n","install","-m","600","-o","65534","-g","65534",str(key_file),str(authority_secret))
        socket_path=root/"socket"/"authority.sock"
        db_path=root/"database"/"plans.sqlite"
        pid_path=root/"database"/"authority.pid"
        # The CI checkout may be inaccessible to the unprivileged authority
        # account. A disposable executable copy avoids relying on home perms.
        public_binary=root/"taiyaki-authority"
        shutil.copy2(authority_bin,public_binary)
        public_binary.chmod(0o755)
        env={
            **os.environ,
            "TAIYAKI_SOCKET_PATH":str(socket_path),
            "TAIYAKI_DB_FILE":str(db_path),
            "TAIYAKI_SECRET_FILE":str(authority_secret),
            "TAIYAKI_CLIENT_UID":str(os.getuid()),
        }
        script='echo $$ > "$1"; exec "$2"'
        assignments=[f"{name}={env[name]}" for name in (
            "TAIYAKI_SOCKET_PATH","TAIYAKI_DB_FILE","TAIYAKI_SECRET_FILE","TAIYAKI_CLIENT_UID")]
        server=subprocess.Popen(["sudo","-n","-u","nobody","env",*assignments,
                                 "/bin/sh","-c",script,"sh",str(pid_path),str(public_binary)],
                                stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
        try:
            for _ in range(150):
                if socket_path.exists():break
                if server.poll() is not None:
                    raise AssertionError("authority exited: "+server.stderr.read())
                time.sleep(0.04)
            assert socket_path.exists(),"authority socket missing"
            assert socket_path.stat().st_mode & 0o777 == 0o666
            assert run("sudo","-n","stat","-c","%a",str(db_path)).stdout.strip()=="600"
            try:
                db_path.read_bytes()
                raise AssertionError("client unexpectedly read private SQLite database")
            except PermissionError:
                pass

            # An arbitrary *other* Unix user cannot connect successfully,
            # including before being told the HMAC challenge.
            denied_script=("import socket,sys,json; s=socket.socket(socket.AF_UNIX); "
                           "s.settimeout(3); s.connect(sys.argv[1]); data=s.recv(1024); "
                           "assert json.loads(data)['code']=='unauthorized',data")
            run("sudo","-n","-u","nobody","python3","-c",denied_script,str(socket_path))

            # One valid proof cannot be replayed on another challenge.
            def frame(stream):
                payload=bytearray()
                while True:
                    char=stream.recv(1)
                    assert char,"authority closed handshake"
                    if char==b"\n":
                        return json.loads(payload.decode())
                    payload.extend(char)

            with socket.socket(socket.AF_UNIX) as first:
                first.settimeout(3)
                first.connect(str(socket_path))
                challenge=frame(first)
                digest=hmac.new(bytes.fromhex(secret.strip()),
                    b"taiyaki-authority-v1"+bytes.fromhex(challenge["nonce"]),hashlib.sha256).hexdigest()
                prior={"mac":digest}
                first.sendall((json.dumps(prior)+"\n").encode())
                first.sendall((json.dumps({"operation":"get"})+"\n").encode())
                assert frame(first)["status"]=="error"  # plan not created yet
            with socket.socket(socket.AF_UNIX) as second:
                second.settimeout(3)
                second.connect(str(socket_path))
                changed=frame(second)
                assert changed["nonce"]!=challenge["nonce"],"nonce reused"
                second.sendall((json.dumps(prior)+"\n").encode())
                second.sendall((json.dumps({"operation":"get"})+"\n").encode())
                rejected=frame(second)
                assert rejected["status"]=="error" and rejected["code"]=="unauthorized",rejected

            bad=Mcp(socket_path,bogus)
            try:
                bad.initialize()
                response=bad.tool(2,"plan_get")
                assert response["isError"] is True,response
                assert response["structuredContent"]["code"]=="unauthorized",response
            finally:bad.close()

            client=Mcp(socket_path,key_file)
            try:
                client.initialize()
                discovery=client.call(2,"tools/list",{})["result"]["tools"]
                assert sorted(tool["name"] for tool in discovery)==[
                    "plan_create","plan_edit","plan_get","plan_review"
                ]
                assert client.tool(3,"plan_get")["structuredContent"]["code"]=="not_found"
                made=client.tool(4,"plan_create",{"title":"Private Plan","description":"isolated"})
                assert made["structuredContent"]["revision"]==1,made
                step=client.tool(5,"plan_edit",{"expected_revision":1,"edit":{"action":"add_item","item":{
                    "title":"Check phase","disposition":"ready",
                    "required_gate_ids":["cargo-test"],"acceptance_criteria":["Must pass tests"]}}})
                assert step["structuredContent"]["revision"]==2,step
                stale=client.tool(6,"plan_review",{"expected_revision":1,"action":"submit"})
                assert stale["structuredContent"]["code"]=="revision_conflict",stale
                done=client.tool(7,"plan_review",{"expected_revision":2,"action":"submit"})
                assert done["structuredContent"]["review_state"]=="submitted",done
                denied=client.tool(8,"plan_edit",{"expected_revision":3,"edit":{"action":"set_details","title":"Overwritten","description":""}})
                assert denied["structuredContent"]["code"]=="review_locked",denied
                forbidden=client.call(9,"tools/call",{"name":"approve","arguments":{}})
                assert "error" in forbidden or forbidden.get("result",{}).get("isError")
            finally:client.close()

            restarted=Mcp(socket_path,key_file)
            try:
                restarted.initialize()
                data=restarted.tool(11,"plan_get")["structuredContent"]
                assert data["revision"]==3 and data["review_state"]=="submitted"
            finally:restarted.close()
            assert not (root/"plan.json").exists()
            assert run("sudo","-n","test","-f",str(db_path)).returncode == 0
            print("PASS: separate UIDs, kernel peer denial, wrong HMAC, 4 MCP tools, SQLite persistence, revisions, review lock")
        finally:
            try:
                pid=run("sudo","-n","cat",str(pid_path)).stdout.strip()
                if pid.isdecimal():
                    subprocess.run(["sudo","-n","kill","-TERM",pid],capture_output=True,timeout=3)
            except (subprocess.CalledProcessError,subprocess.TimeoutExpired):pass
            try:server.communicate(timeout=4)
            except subprocess.TimeoutExpired:
                server.kill()
                server.communicate()
            # cleanup non-owned state directory using sudo only in disposable root
            subprocess.run(["sudo","-n","rm","-rf",str(root/"socket"),str(root/"database")],check=True,capture_output=True)

if __name__=="__main__":
    main()

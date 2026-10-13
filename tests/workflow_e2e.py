#!/usr/bin/env python3
"""Real workflow: agent -> authority -> independent verifier -> SQLite DONE.

Requires passwordless sudo. Disposable 3-UID fixture, no service installed.
"""
import os,sys,time,json,hashlib,hmac,socket,shutil,subprocess,tempfile,uuid
from pathlib import Path
from authority_e2e import Mcp,run

authority_bin,mcp_bin,verifier_bin=sys.argv[1:4]
verifier_user="daemon"
verifier_uid=int(run("id","-u",verifier_user).stdout.strip())
assert verifier_uid not in (65534,os.getuid())
def sha(raw):return hashlib.sha256(raw).hexdigest()
def pinned(gate):return sha(json.dumps(gate,sort_keys=True,separators=(",",":"),ensure_ascii=False).encode())
def witness_test(path,body):
    scratch=Path("/tmp")/("taiyaki-case-"+uuid.uuid4().hex)
    scratch.write_text(body)
    run("sudo","-n","install","-o",verifier_user,"-g",str(verifier_uid),"-m","600",str(scratch),str(path))
    scratch.unlink()
def as_verifier(*args):
    return run("sudo","-n","-u",verifier_user,"env","HOME=/tmp","GIT_CONFIG_GLOBAL=/dev/null",*args)
def make_spec(test_id,title,filename):
    return {
      "test_type":"functional","test_id":test_id,"test_name":title,
      "test_description":"Runs a real Python assertion against the implementation",
      "test_goal":"Detect an incorrect implementation before marking DONE",
      "precondition":"Reviewed immutable verifier-owned test snapshot",
      "expected_result":"An actual test process exits zero and emits the required marker",
      "test_code":"tests/"+filename,"test_status":"not_run"
    }
def verify_catalog(root,catalog_path,definitions,markers):
    cases=[]
    for definition,marker in zip(definitions,markers):
        src=root/definition["test_code"]
        cases.append({
            "test_id":definition["test_id"],"test_code":definition["test_code"],
            "test_code_sha256":run("sudo","-n","sha256sum",str(src)).stdout.split()[0],
            "test_definition_sha256":pinned(definition),
            "executable":"/usr/bin/python3",
            "args":[definition["test_code"]],
            "success_marker":marker,"timeout_seconds":8
        })
    temp=root.parent/("pending-catalog-"+uuid.uuid4().hex)
    temp.write_text(json.dumps({"source_root":str(root),"cases":cases}))
    run("sudo","-n","install","-o",verifier_user,"-g",str(verifier_uid),"-m","600",str(temp),str(catalog_path))
    temp.unlink()
def git_commit(root,message):
    as_verifier("git","-C",str(root),"add",".")
    as_verifier("git","-C",str(root),"-c","user.name=Taiyaki Fixtures",
                "-c","user.email=fixtures@invalid.test","commit","-qm",message)
def call_ok(client,index,name,data):
    response=client.tool(index,name,data)
    assert response["isError"]==False,(name,response)
    return response["structuredContent"]
def work_status(client,n):
    return call_ok(client,n,"work_status",{})
def attempt_forged_pass(socket_path,client_key,expected_revision,item_id):
    # Correct MCP-client key, but incorrect OS identity for evidence submission.
    with socket.socket(socket.AF_UNIX) as connection:
        connection.settimeout(4)
        connection.connect(str(socket_path))
        source=connection.makefile("rb")
        challenge=json.loads(source.readline())
        key=bytes.fromhex(client_key.read_text().strip())
        proof=hmac.new(key,b"taiyaki-authority-v1"+bytes.fromhex(challenge["nonce"]),hashlib.sha256).hexdigest()
        connection.sendall((json.dumps({"mac":proof})+"\n").encode())
        forged={"operation":"verify_commit","expected_revision":expected_revision,
            "item_id":item_id,"source_tree_sha256":"0"*64,
            "results":[{"test_id":"case-alpha","test_code_sha256":"0"*64,
                         "code_valid":True,"passed":True}]}
        connection.sendall((json.dumps(forged)+"\n").encode())
        answer=json.loads(source.readline())
        assert answer["status"]=="error" and answer["code"]=="forbidden",answer

def external_verifier(root,socket_path,secret_path,verifier_executable,catalog):
    envargs=["TAIYAKI_SOCKET_PATH="+str(socket_path),
             "TAIYAKI_SECRET_FILE="+str(secret_path),
             "TAIYAKI_TEST_CATALOG="+str(catalog)]
    return as_verifier("env",*envargs,str(verifier_executable),"--once")
with tempfile.TemporaryDirectory(prefix="taiyaki-workflow-e2e-") as temp:
    root=Path(temp);root.chmod(0o711)
    for path,perm,owner in (("socket",0o711,"nobody"),("database",0o700,"nobody"),
                            ("approved",0o700,verifier_user),("verifier-private",0o700,verifier_user)):
        dest=root/path;dest.mkdir(mode=perm)
        run("sudo","-n","chown",owner+":"+owner,str(dest))
    approved=root/"approved"
    as_verifier("mkdir","-m","700",str(approved/"tests"))
    witness_test(approved/"implementation.txt","NOT_READY\n")
    witness_test(approved/"tests"/"alpha.py",'''
from pathlib import Path
assert Path("implementation.txt").read_text().strip()
print("ALPHA_TEST_PASSED")
''')
    witness_test(approved/"tests"/"beta.py",'''
from pathlib import Path
assert Path("implementation.txt").read_text().strip()=="READY"
print("BETA_TEST_PASSED")
''')
    as_verifier("git","-C",str(approved),"init","-q")
    git_commit(approved,"initial immutable candidate")
    master=root/"client.key"
    master.write_text(os.urandom(32).hex()+"\n");master.chmod(0o600)
    verifier_master=root/"verifier-private"/"verifier.key"
    scratch=root/"temp-verifier-key"
    scratch.write_text(os.urandom(32).hex()+"\n")
    run("sudo","-n","install","-o",verifier_user,"-g",str(verifier_uid),"-m","600",str(scratch),str(verifier_master))
    scratch.unlink()
    authority_key=root/"database"/"authority.key"
    verifier_authority_key=root/"database"/"verifier.key"
    run("sudo","-n","install","-o","nobody","-g","nobody","-m","600",str(master),str(authority_key))
    run("sudo","-n","install","-o","nobody","-g","nobody","-m","600",str(verifier_master),str(verifier_authority_key))
    socket_path=root/"socket"/"authority.sock"
    data_path=root/"database"/"plans.sqlite"
    pid_path=root/"database"/"authority.pid"
    copy_auth=root/"taiyaki-authority";shutil.copy2(authority_bin,copy_auth);copy_auth.chmod(0o755)
    copy_verify=root/"taiyaki-verifier";shutil.copy2(verifier_bin,copy_verify);copy_verify.chmod(0o755)
    catalog_path=root/"verifier-private"/"catalog.json"
    cfg={
      "TAIYAKI_SOCKET_PATH":str(socket_path),"TAIYAKI_DB_FILE":str(data_path),
      "TAIYAKI_SECRET_FILE":str(authority_key),"TAIYAKI_CLIENT_UID":str(os.getuid()),
      "TAIYAKI_VERIFIER_UID":str(verifier_uid),"TAIYAKI_VERIFIER_SECRET_FILE":str(verifier_authority_key)
    }
    supervisor=subprocess.Popen(["sudo","-n","-u","nobody","env",*[f"{k}={v}" for k,v in cfg.items()],
        "/bin/sh","-c",'echo $$ > "$1"; exec "$2"',"sh",str(pid_path),str(copy_auth)],
        stdout=subprocess.DEVNULL,stderr=subprocess.PIPE,text=True)
    try:
        for _ in range(150):
            if socket_path.exists():break
            if supervisor.poll() is not None:raise AssertionError(supervisor.stderr.read())
            time.sleep(.04)
        assert socket_path.exists()
        client=Mcp(socket_path,master)
        try:
            client.initialize()
            res=call_ok(client,2,"plan_create",{"title":"Taiyaki verifies Taiyaki"})
            version=res["revision"]
            # Priority inversion exercise: higher priority beta depends on alpha.
            ids=[]
            for idx,(title,gate,priority,dependencies) in enumerate((
                ("Foundation","case-alpha","normal",[]),
                ("Depends on foundation","case-beta","critical",[0]),
            )):
                res=call_ok(client,3+idx,"plan_edit",{
                    "expected_revision":version,
                    "edit":{"action":"add_item","item":{
                        "title":title,"priority":priority,"disposition":"ready",
                        "depends_on":[ids[i] for i in dependencies],
                        "acceptance_criteria":["Actual independently executed test succeeds"],
                        "required_gate_ids":[gate]
                    }}})
                version=res["revision"]
                ids.append(res["work_items"][-1]["id"])
            res=call_ok(client,6,"plan_review",{"expected_revision":version,"action":"submit"})
            assert res["review_state"]=="submitted"
            flow=work_status(client,7);revision=flow["revision"]
            alpha=make_spec("case-alpha","Alpha functional assertion","alpha.py")
            beta=make_spec("case-beta","Beta functional assertion","beta.py")
            defs=[alpha,beta]
            markers=["ALPHA_TEST_PASSED","BETA_TEST_PASSED"]
            verify_catalog(approved,catalog_path,defs,markers)
            for index,definition in enumerate(defs,8):
                flow=call_ok(client,index,"test_define",{"expected_revision":revision,"gate":definition})
                revision=flow["revision"]
            # Client cannot forge status PASS.
            forged=dict(alpha,test_status="pass")
            invalid=client.tool(12,"test_define",{"expected_revision":revision,"gate":forged})
            assert invalid["isError"] and invalid["structuredContent"]["code"]=="workflow_rejected",invalid
            # Client cannot invoke internal verifier operations on authenticated MCP principal.
            next_data=call_ok(client,13,"work_next",{"expected_revision":revision,"agent":"model-1"})
            assignment=next_data["assignment"]
            assert assignment["work_item"]["id"]==ids[0]
            revision=next_data["workflow"]["revision"]
            assert all(r["lease"] is None for r in next_data["workflow"]["work"].values())
            forged_report=client.tool(14,"work_report",{
                "expected_revision":revision,"item_id":ids[0],"lease":str(uuid.uuid4()),"notes":"Fake"})
            assert forged_report["isError"]
            flow=call_ok(client,15,"work_report",{
                "expected_revision":revision,"item_id":ids[0],"lease":assignment["lease"],"notes":"Implementation prepared"})
            revision=flow["revision"]
            assert flow["work"][ids[0]]["phase"]=="verifying"
            attempt_forged_pass(socket_path,master,revision,ids[0])
            assert work_status(client,115)["work"][ids[0]]["phase"]=="verifying"
            external_verifier(approved,socket_path,verifier_master,copy_verify,catalog_path)
            flow=work_status(client,16);revision=flow["revision"]
            assert flow["work"][ids[0]]["phase"]=="done",flow
            assert flow["receipts"][ids[0]][0]["passed"] is True
            first_tree=flow["receipts"][ids[0]][0]["source_tree_sha256"]
            assert first_tree
            next_data=call_ok(client,17,"work_next",{"expected_revision":revision,"agent":"model-1"})
            assert next_data["assignment"]["work_item"]["id"]==ids[1]
            flow=call_ok(client,18,"work_report",{
                "expected_revision":next_data["workflow"]["revision"],
                "item_id":ids[1],"lease":next_data["assignment"]["lease"],"notes":"Implementation pending"})
            external_verifier(approved,socket_path,verifier_master,copy_verify,catalog_path)
            flow=work_status(client,19)
            assert flow["work"][ids[1]]["phase"]=="ready",flow
            assert flow["receipts"][ids[1]][0]["passed"] is False,flow
            # An implementation change invalidates *all* old source-bound PASS
            # evidence, including the already-DONE prerequisite.
            witness_test(approved/"implementation.txt","READY\n")
            git_commit(approved,"reviewed corrected implementation")
            external_verifier(approved,socket_path,verifier_master,copy_verify,catalog_path)
            stale=work_status(client,20)
            assert stale["work"][ids[0]]["phase"]=="ready",stale
            assert stale["work"][ids[1]]["phase"]=="ready",stale
            assert stale["gates"]["case-alpha"]["test_status"]=="not_run"
            again=call_ok(client,21,"work_next",{"expected_revision":stale["revision"],"agent":"model-1"})
            assert again["assignment"]["work_item"]["id"]==ids[0]
            call_ok(client,22,"work_report",{
                "expected_revision":again["workflow"]["revision"],"item_id":ids[0],
                "lease":again["assignment"]["lease"],"notes":"Reverify prerequisite on new tree"})
            external_verifier(approved,socket_path,verifier_master,copy_verify,catalog_path)
            checked=work_status(client,23)
            assert checked["work"][ids[0]]["phase"]=="done"
            assert checked["receipts"][ids[0]][0]["source_tree_sha256"]!=first_tree
            again=call_ok(client,24,"work_next",{"expected_revision":checked["revision"],"agent":"model-1"})
            assert again["assignment"]["work_item"]["id"]==ids[1]
            call_ok(client,25,"work_report",{
                "expected_revision":again["workflow"]["revision"],"item_id":ids[1],
                "lease":again["assignment"]["lease"],"notes":"Fixed implementation, independently reviewed"})
            external_verifier(approved,socket_path,verifier_master,copy_verify,catalog_path)
            final=work_status(client,26)
            assert final["work"][ids[1]]["phase"]=="done"
            assert final["receipts"][ids[1]][0]["passed"] is True
            assert final["receipts"][ids[0]][0]["plan_revision"]==final["plan_revision"]
            assert final["receipts"][ids[0]][0]["source_tree_sha256"]==final["receipts"][ids[1]][0]["source_tree_sha256"]
            print("PASS: 3 Unix UIDs, real MCP WorkItems, dependency gating, test catalog approval, real failure/retry, fresh PASS receipts, durable DONE")
        finally:client.close()
    finally:
        try:
            pid=run("sudo","-n","cat",str(pid_path)).stdout.strip()
            if pid.isdecimal():subprocess.run(["sudo","-n","kill","-TERM",pid],capture_output=True,timeout=3)
        except (subprocess.CalledProcessError,subprocess.TimeoutExpired):pass
        try:supervisor.communicate(timeout=4)
        except subprocess.TimeoutExpired:supervisor.kill();supervisor.communicate()
        subprocess.run(["sudo","-n","rm","-rf",str(root/"socket"),str(root/"database"),str(root/"approved"),str(root/"verifier-private")],check=True,capture_output=True)

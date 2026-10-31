# A minimal MCP server over stdio, for tests. It offers two tools: `echo` (read-only) and
# `write_note` (not annotated, so treated as changing something).
import json, sys

TOOLS = [
    {"name": "echo", "description": "Repeat the text back", "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}},
     "annotations": {"readOnlyHint": True}},
    {"name": "write_note", "description": "Save a note to the notebook", "inputSchema": {"type": "object", "properties": {"note": {"type": "string"}}}},
]

for line in sys.stdin:
    msg = json.loads(line)
    if "id" not in msg:
        continue  # notifications get no answer
    method, params, rid = msg.get("method"), msg.get("params", {}), msg["id"]
    if method == "initialize":
        result = {"protocolVersion": params.get("protocolVersion"), "capabilities": {"tools": {}}, "serverInfo": {"name": "fake", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": TOOLS}
    elif method == "tools/call":
        name, args = params.get("name"), params.get("arguments", {})
        if name == "echo":
            result = {"content": [{"type": "text", "text": args.get("text", "")}], "isError": False}
        elif name == "write_note":
            result = {"content": [{"type": "text", "text": "saved: " + args.get("note", "")}], "isError": False}
        else:
            result = {"content": [{"type": "text", "text": "no such tool"}], "isError": True}
    else:
        print(json.dumps({"jsonrpc": "2.0", "id": rid, "error": {"code": -32601, "message": "unknown method"}}), flush=True)
        continue
    print(json.dumps({"jsonrpc": "2.0", "id": rid, "result": result}), flush=True)

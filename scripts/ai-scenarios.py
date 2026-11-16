#!/usr/bin/env python3
"""The brief's three AI scenarios, end to end, through `craftworks mcp` (Phase 7).

This script plays the AI's part with fixed steps, so it can run unattended. The real
acceptance is Claude Code doing the same with only this server connected (docs/ai.md).

    scripts/ai-scenarios.py [--binary target/measure/craftworks] [--dir plan/ai-e2e] [--no-window]

1. Read the Q3 figures from budget.xlsx and write them into a table in report.docx while it's
   open in a Craftworks tab: the tab changes live, and Undo takes the change back. The same for
   an open PDF (H7): a page rotated live by PdfCraft's own tool, then undone.
2. Build a deck from report.docx.
3. Compare the amounts in contract.pdf with payments.xlsx.

Fixtures are made by the connector itself (headless, project permission edit_save), then the
permission is set to `edit` for scenario 1, so the live edit waits for review.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


class Mcp:
    def __init__(self, binary, project, env):
        self.p = subprocess.Popen([binary, "mcp", "--project", project, "--all-tools"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, env=env, text=True)
        self.next = 0
        init = self.rpc("initialize", {"protocolVersion": "2025-06-18", "capabilities": {}, "clientInfo": {"name": "ai-scenarios"}})
        self.instructions = init["instructions"]
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "method": "notifications/initialized"}) + "\n")
        self.p.stdin.flush()

    def rpc(self, method, params=None):
        self.next += 1
        self.p.stdin.write(json.dumps({"jsonrpc": "2.0", "id": self.next, "method": method, "params": params or {}}) + "\n")
        self.p.stdin.flush()
        reply = json.loads(self.p.stdout.readline())
        if "error" in reply:
            raise RuntimeError(f"{method}: {reply['error']}")
        return reply["result"]

    def tool(self, name, **args):
        r = self.rpc("tools/call", {"name": name, "arguments": args})
        text = r["content"][0].get("text", "") if r.get("content") else ""
        if r.get("isError"):
            raise RuntimeError(f"{name}: {text}")
        if r.get("structuredContent") is not None:
            return r["structuredContent"]
        try:
            return json.loads(text)
        except ValueError:
            return text

    def close(self):
        self.p.stdin.close()
        self.p.wait(timeout=30)


def set_permission(project, perm):
    path = os.path.join(project, ".craftworks", "project.json")
    with open(path) as f:
        p = json.load(f)
    p.setdefault("ai", {})["permission"] = perm
    with open(path, "w") as f:
        json.dump(p, f, indent=2)


results = []


def check(ok, what):
    results.append((ok, what))
    print(("PASS " if ok else "FAIL ") + what, flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", default=os.path.join(ROOT, "target/measure/craftworks"))
    ap.add_argument("--dir", default=os.path.join(ROOT, "plan/ai-e2e"))
    ap.add_argument("--no-window", action="store_true", help="skip the live part of scenario 1")
    a = ap.parse_args()
    project = os.path.abspath(os.path.join(a.dir, "project"))
    settings = os.path.abspath(os.path.join(a.dir, "settings"))
    shutil.rmtree(a.dir, ignore_errors=True)
    os.makedirs(project)
    os.makedirs(settings)
    env = dict(os.environ, CRAFTWORKS_SETTINGS_DIR=settings)

    # ── Fixtures, made by the connector (headless, edit_save) ──
    m = Mcp(a.binary, project, env)
    set_permission(project, "edit_save")
    m.tool("sheet.write_range", file="budget.xlsx", range="A1", values=[["Region", "Q3"], ["North", 1200], ["South", 1350], ["East", 980], ["Total", "=SUM(B2:B4)"]])
    m.tool("project_save", file="budget.xlsx")
    m.tool("sheet.write_range", file="payments.xlsx", range="A1", values=[["Date", "Payee", "Amount"], ["2026-10-02", "Acme Services", 1200], ["2026-10-30", "Acme Services", 1350]])
    m.tool("project_save", file="payments.xlsx")
    m.tool("doc.execute", file="report.docx", command="text.insert", params={"text": "Q3 report\nRevenue grew in every region this quarter. The figures are below.\n"})
    m.tool("project_save", file="report.docx")
    m.close()
    make_pdf(a.binary, project, env)
    files = sorted(os.listdir(project))
    check(all(f in files for f in ["budget.xlsx", "payments.xlsx", "report.docx", "contract.pdf"]), f"fixtures made by the connector ({', '.join(f for f in files if not f.startswith('.'))})")

    # ── Scenario 1: live, in the open tab ──
    set_permission(project, "edit")
    window = None
    if not a.no_window:
        window = subprocess.Popen([a.binary, os.path.join(project, "report.docx")], env=env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        for _ in range(300):
            if os.path.exists(os.path.join(settings, "live.token")):
                break
            time.sleep(0.1)
        time.sleep(2.0)
    m = Mcp(a.binary, project, env)
    listed = m.tool("project_files")
    report_open = any(f["file"] == "report.docx" and f["open_in_window"] for f in listed["files"])
    if window:
        check(report_open, "report.docx is open in a Craftworks tab")
    q3 = m.tool("sheet.read_range", file="budget.xlsx", range="A1:B5")
    rows = q3.get("values") if isinstance(q3, dict) else None
    check(bool(rows) and any("1350" in json.dumps(r) for r in rows), f"read Q3 figures from budget.xlsx ({json.dumps(rows)[:120] if rows else q3})")
    table = [[str(c) if c is not None else "" for c in r] for r in rows] if rows else [["Region", "Q3"]]
    if window:
        m.tool("doc.insert_table", file="report.docx", values=table, header=True)
        text = json.dumps(m.tool("doc.inspect_document", file="report.docx"))
        check("North" in text and "1350" in text, "the table is in report.docx, live in its tab")
        try:
            m.tool("project_save", file="report.docx")
            check(False, "an `edit` project doesn't save the AI's edit")
        except RuntimeError as e:
            check("review" in str(e), "an `edit` project doesn't save the AI's edit: it waits for review")
        m.tool("doc.execute", file="report.docx", command="edit.undo", params={})
        text = json.dumps(m.tool("doc.inspect_document", file="report.docx"))
        check("1350" not in text, "Undo takes the table back")
        m.tool("doc.execute", file="report.docx", command="edit.redo", params={})
        # H7: an open PDF, edited live by PdfCraft's own tools, and undone.
        m.tool("project_open", file="contract.pdf")
        rot = lambda: m.tool("pdf.doc_info", file="contract.pdf")["pages"][0]["rotation"]
        before = rot()
        m.tool("pdf.page_rotate", file="contract.pdf", pages=[1], degrees=90)
        after = rot()
        check(after == (before + 90) % 360, f"contract.pdf, open in a tab, is rotated live by PdfCraft's tool ({before}° → {after}°)")
        m.tool("pdf.edit_undo", file="contract.pdf")
        check(rot() == before, "and PdfCraft's Undo takes it back")
    else:
        print("SKIP the live part of scenario 1 (--no-window)")
    m.close()

    # ── Scenario 2: a deck from report.docx (headless, saved: edit_save) ──
    set_permission(project, "edit_save")
    m = Mcp(a.binary, project, env)
    report = m.tool("project_read", file="report.docx")
    paras = [p["text"] for p in report["parts"] if p["text"].strip()]
    m.tool("deck.add_slide", file="report deck.pptx", layout="title", title=paras[0] if paras else "Report")
    for p in paras[1:]:
        m.tool("deck.add_slide", file="report deck.pptx", layout="titleAndContent", title="Summary", body=p)
    m.tool("project_save", file="report deck.pptx")
    deck = m.tool("project_read", file="report deck.pptx")
    deck_text = json.dumps(deck)
    check(os.path.exists(os.path.join(project, "report deck.pptx")) and "Revenue grew" in deck_text, f"built report deck.pptx from report.docx ({len(deck['parts'])} parts)")

    # ── Scenario 3: contract.pdf against payments.xlsx ──
    contract = json.dumps(m.tool("project_read", file="contract.pdf"))
    pay = m.tool("sheet.read_range", file="payments.xlsx", range="A2:C3")
    paid = [r[2] for r in pay.get("values", [])]
    found = [str(int(x)) for x in paid if isinstance(x, (int, float))]
    in_contract = [x for x in found if f"{x[:-3]},{x[-3:]}" in contract or x in contract]
    total = sum(int(x) for x in found)
    check(len(found) == 2 and len(in_contract) == 2 and "2,550" in contract and total == 2550, f"payments {found} (total {total}) match the contract's instalments and fee")
    hits = m.tool("project_search", query="Acme")
    check({h["file"] for h in hits["hits"]} >= {"contract.pdf", "payments.xlsx"}, "project_search finds Acme in the PDF and the sheet")
    m.close()

    if window:
        window.terminate()
        window.wait(timeout=10)
    failed = [w for ok, w in results if not ok]
    print(f"\n{'ALL PASS' if not failed else f'{len(failed)} FAILED'} ({len(results)} checks)")
    return 1 if failed else 0


def make_pdf(binary, project, env):
    """contract.pdf, made by PdfCraft through the connector (pdf.doc_create from text, then
    pdf.doc_save)."""
    m = Mcp(binary, project, env)
    text = "Services agreement between Craftworks Ltd and Acme Services. Total fee: 2,550, paid in two instalments of 1,200 and 1,350."
    made = m.tool("pdf.doc_create", **{"from": "text", "text": text})
    doc = made.get("doc") or made.get("id") if isinstance(made, dict) else None
    if doc is None:
        raise SystemExit(f"pdf.doc_create gave no document: {made}")
    m.tool("pdf.doc_save", doc=doc, path="contract.pdf")
    m.close()


if __name__ == "__main__":
    sys.exit(main())

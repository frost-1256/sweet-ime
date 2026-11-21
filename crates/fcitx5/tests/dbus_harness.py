#!/usr/bin/env python3
"""End-to-end check of the fcitx5 addon through fcitx5's D-Bus frontend.

Run it with `crates/fcitx5/tests/run-dbus-harness.sh`, which starts a private
fcitx5 on its own D-Bus session with only Sweet IME enabled, so the desktop's
input method is not touched.
"""

import sys
import time

import dbus
import dbus.mainloop.glib
from gi.repository import GLib

PREEDIT = 1 << 1
FORMATTED_PREEDIT = 1 << 4
SURROUNDING_TEXT = 1 << 6
CLIENT_SIDE_INPUT_PANEL = 1 << 39

SHIFT = 1 << 0
ALT = 1 << 3

KEYSYMS = {
    " ": 0x20,
    "space": 0x20,
    "Return": 0xFF0D,
    "Escape": 0xFF1B,
    "BackSpace": 0xFF08,
    "Tab": 0xFF09,
    "Down": 0xFF54,
    "F7": 0xFFC4,
    "Zenkaku_Hankaku": 0xFF2A,
}

dbus.mainloop.glib.DBusGMainLoop(set_as_default=True)
bus = dbus.SessionBus()

for _ in range(100):
    if bus.name_has_owner("org.fcitx.Fcitx5"):
        break
    time.sleep(0.1)
else:
    sys.exit("fcitx5 did not start")

im = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", "/org/freedesktop/portal/inputmethod"),
                    "org.fcitx.Fcitx.InputMethod1")
path, _uuid = im.CreateInputContext([("program", "sweet-harness")])
ic = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", path), "org.fcitx.Fcitx.InputContext1")
controller = dbus.Interface(bus.get_object("org.fcitx.Fcitx5", "/controller"), "org.fcitx.Fcitx.Controller1")

state = {"commit": "", "preedit": "", "candidates": [], "cursor": -1, "aux": ""}


def on_commit(text):
    state["commit"] += str(text)


def on_preedit(spans, _cursor):
    state["preedit"] = "".join(str(s) for s, _ in spans)


def on_ui(preedit, _cursor, _aux_up, aux_down, candidates, index, *_rest):
    state["candidates"] = [str(text) for _, text in candidates]
    state["cursor"] = int(index)
    state["aux"] = "".join(str(s) for s, _ in aux_down)


ic.connect_to_signal("CommitString", on_commit)
ic.connect_to_signal("UpdateFormattedPreedit", on_preedit)
ic.connect_to_signal("UpdateClientSideUI", on_ui)

ic.SetCapability(dbus.UInt64(PREEDIT | FORMATTED_PREEDIT | CLIENT_SIDE_INPUT_PANEL))
ic.FocusIn()
controller.SetCurrentIM("sweetime")


def pump(seconds=0.05):
    end = time.time() + seconds
    ctx = GLib.MainContext.default()
    while time.time() < end:
        while ctx.pending():
            ctx.iteration(False)
        time.sleep(0.005)


def key(name, mods=0):
    sym = KEYSYMS.get(name, ord(name[0]) if len(name) == 1 else None)
    eaten = ic.ProcessKeyEvent(dbus.UInt32(sym), dbus.UInt32(0), dbus.UInt32(mods), False, dbus.UInt32(0))
    ic.ProcessKeyEvent(dbus.UInt32(sym), dbus.UInt32(0), dbus.UInt32(mods), True, dbus.UInt32(0))
    pump()
    return bool(eaten)


def type_str(text):
    for ch in text:
        key(ch, SHIFT if ch.isupper() else 0)


def clear():
    state.update(commit="", preedit="", candidates=[], cursor=-1, aux="")


failures = 0


def check(name, got, want):
    global failures
    ok = got == want
    failures += not ok
    print(f"{'ok  ' if ok else 'FAIL'} {name}: {got!r}" + ("" if ok else f" (want {want!r})"))


pump(0.3)
check("current input method", str(controller.CurrentInputMethod()), "sweetime")

type_str("kyouhaiitenkidesune")
check("live preedit", state["preedit"], "今日はいい天気ですね")
key("Return")
check("commit live conversion", state["commit"], "今日はいい天気ですね")
check("preedit cleared", state["preedit"], "")

clear()
type_str("kisha")
key("space")
key("space")
check("candidate list shown", len(state["candidates"]) > 1, True)
check("second candidate selected", state["cursor"], 1)
check("footer", state["aux"].startswith("2 / "), True)
picked = state["candidates"][1] if len(state["candidates"]) > 1 else ""
key("Return")
check("commit chosen candidate", state["commit"], picked)

clear()
type_str("arigat")
check("suggestions shown", "ありがとう" in state["candidates"], True)
check("suggestions not selected", state["cursor"], -1)
key("Tab")
key("Return")
check("commit suggestion", state["commit"].startswith("ありがとう"), True)

clear()
type_str("ko-hi-")
key("F7")
key("Return")
check("katakana (F7)", state["commit"], "コーヒー")

clear()
type_str("kana")
key("Escape")
key("Escape")
check("escape cancels", (state["preedit"], state["commit"]), ("", ""))

clear()
check("zenkaku/hankaku turns off", key("Zenkaku_Hankaku"), True)
check("off passes keys through", key("a"), False)
key("Zenkaku_Hankaku")
type_str("kana")
key("Return")
check("on again", state["commit"], "かな")

clear()
key("T", SHIFT)
type_str("est")
key("space")
check("shift starts English", state["commit"], "Test ")

clear()
type_str("nginx")
key("space")
key("Return")
check("technical spelling", state["commit"], "Nginx")

print("ALL PASSED" if failures == 0 else f"{failures} FAILED")
sys.exit(1 if failures else 0)

import { useEffect, useRef, useCallback } from "react";
import { listen } from "@tauri-apps/api/event";

export interface GamepadActions {
  onUp: () => void;
  onDown: () => void;
  onLeft: () => void;
  onRight: () => void;
  onConfirm: () => void;
  onBack: () => void;
  onDelete: () => void;
  onAdd: () => void;
  onLB?: () => void;
  onRB?: () => void;
  onLT?: () => void;
  onRT?: () => void;
  onSelect?: () => void;
}

function dispatch(a: GamepadActions, btn: string) {
  switch (btn) {
    case "DPadUp":    a.onUp(); break;
    case "DPadDown":  a.onDown(); break;
    case "DPadLeft":  a.onLeft(); break;
    case "DPadRight": a.onRight(); break;
    case "A":         a.onConfirm(); break;
    case "B":         a.onBack(); break;
    case "Y":         a.onAdd(); break;
    case "X":         a.onDelete(); break;
    case "LB":        a.onLB?.(); break;
    case "RB":        a.onRB?.(); break;
    case "LT":        a.onLT?.(); break;
    case "RT":        a.onRT?.(); break;
    case "Select":    a.onSelect?.(); break;
  }
}

export function useGamepad(actions: GamepadActions, enabled = true) {
  const actionsRef = useRef(actions);
  actionsRef.current = actions;

  // Track which buttons Tauri already handled this frame to avoid double-fire
  const tauriButtons = useRef<Set<string>>(new Set());

  // Tauri event path (from Rust gilrs) — always active, handles whatever Rust sends
  useEffect(() => {
    if (!enabled) return;
    const unlisten = listen<string>("gamepad-button", (event) => {
      tauriButtons.current.add(event.payload);
      dispatch(actionsRef.current, event.payload);
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [enabled]);

  // Browser Gamepad API — always polls, but skips buttons Tauri already handled
  const prevButtons = useRef<boolean[]>([]);
  const prevAxes = useRef<number[]>([]);
  const axisCooldown = useRef(false);
  const axisDeadzone = 0.5;

  // Map browser button index to our event name
  // Note: Start button (index 9) is intentionally excluded - Rust handles it for overlay toggle
  const BTN_MAP: [number, string][] = [
    [0, "A"], [1, "B"], [2, "X"], [3, "Y"],
    [4, "LB"], [5, "RB"], [6, "LT"], [7, "RT"],
    [8, "Select"],
    [12, "DPadUp"], [13, "DPadDown"], [14, "DPadLeft"], [15, "DPadRight"],
  ];

  const poll = useCallback(() => {
    const gamepads = navigator.getGamepads();
    const gp = gamepads[0];
    if (!gp) return;

    const a = actionsRef.current;
    const prev = prevButtons.current;
    const handled = tauriButtons.current;

    for (const [idx, name] of BTN_MAP) {
      if (gp.buttons[idx]?.pressed && !prev[idx]) {
        // Skip if Tauri already dispatched this button
        if (handled.has(name)) continue;
        dispatch(a, name);
      }
    }

    // Clear Tauri set each frame so it only dedupes within the same press
    handled.clear();

    if (!axisCooldown.current) {
      const lx = gp.axes[0] ?? 0;
      const ly = gp.axes[1] ?? 0;
      const plx = prevAxes.current[0] ?? 0;
      const ply = prevAxes.current[1] ?? 0;
      let moved = false;
      if (ly < -axisDeadzone && ply >= -axisDeadzone) { a.onUp(); moved = true; }
      if (ly > axisDeadzone && ply <= axisDeadzone) { a.onDown(); moved = true; }
      if (lx < -axisDeadzone && plx >= -axisDeadzone) { a.onLeft(); moved = true; }
      if (lx > axisDeadzone && plx <= axisDeadzone) { a.onRight(); moved = true; }
      if (moved) {
        axisCooldown.current = true;
        setTimeout(() => { axisCooldown.current = false; }, 180);
      }
    }

    prevButtons.current = gp.buttons.map((b) => b.pressed);
    prevAxes.current = [...gp.axes];
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const onConn = (e: GamepadEvent) => console.log("🎮 Gamepad connected:", e.gamepad.id);
    const onDisc = (e: GamepadEvent) => console.log("🎮 Gamepad disconnected:", e.gamepad.id);
    window.addEventListener("gamepadconnected", onConn);
    window.addEventListener("gamepaddisconnected", onDisc);

    let rafId: number;
    const loop = () => { poll(); rafId = requestAnimationFrame(loop); };
    rafId = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(rafId);
      window.removeEventListener("gamepadconnected", onConn);
      window.removeEventListener("gamepaddisconnected", onDisc);
    };
  }, [poll, enabled]);
}

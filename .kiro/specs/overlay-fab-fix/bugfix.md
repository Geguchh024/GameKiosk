# Bugfix Requirements Document

## Introduction

Two bugs affect the overlay FAB (floating action button) system in the Tauri game launcher:

1. The FAB window (`fab.html`) renders a visible transparent/semi-transparent background area around the icon button, even though the window is configured as transparent. Only the icon itself should be visible — no container, border, or background rectangle.

2. The controller Home/Guide button (`Button::Mode` in gilrs) is intended to toggle the overlay open/close, but it does not work reliably. The Rust `start_gamepad_thread` already maps `Button::Mode` to toggle the overlay, however the `gilrs` library may not detect the Home/Guide button on many controllers (it is a known limitation). Meanwhile, the frontend `useGamepad.ts` maps the Start button (`Button::Start` / browser gamepad index 9) to `onStart`, which currently opens the edit modal in the main app — not the overlay. The user wants the Home button to toggle the overlay, and since `Button::Mode` detection is unreliable in gilrs, the Start button should be repurposed as the overlay toggle trigger.

## Bug Analysis

### Current Behavior (Defect)

1.1 WHEN the FAB window is displayed THEN the system shows a visible rectangular background/container area around the icon button, despite the window having `transparent: true` and CSS `background: transparent`

1.2 WHEN the user presses the Home/Guide button on a controller THEN the system fails to toggle the overlay because `gilrs` does not reliably detect `Button::Mode` on many controller types

1.3 WHEN the user presses the Start button on a controller while in the main launcher THEN the system triggers `onStart` which opens the edit modal, instead of toggling the overlay

### Expected Behavior (Correct)

2.1 WHEN the FAB window is displayed THEN the system SHALL render only the icon image with no visible background, border, or container rectangle — the window shall be fully transparent except for the icon pixels

2.2 WHEN the user presses the Start button on a controller (during gameplay, when FAB/overlay is active) THEN the system SHALL toggle the overlay open/close, matching the intended Home button behavior

2.3 WHEN the user presses the Start button on a controller while in the main launcher THEN the system SHALL toggle the overlay (or be handled as a no-op if the overlay system is not active), rather than opening the edit modal

### Unchanged Behavior (Regression Prevention)

3.1 WHEN the user clicks the FAB icon with the mouse THEN the system SHALL CONTINUE TO invoke `show_overlay` and display the overlay panel

3.2 WHEN the user presses Ctrl+Shift+G THEN the system SHALL CONTINUE TO toggle the overlay via the global shortcut

3.3 WHEN the user presses D-pad, A, B, X, Y, LB, RB, LT, RT buttons on a controller THEN the system SHALL CONTINUE TO emit the corresponding `gamepad-button` events and handle navigation/actions as before

3.4 WHEN the user clicks the close button on the overlay panel THEN the system SHALL CONTINUE TO hide the overlay and show the FAB

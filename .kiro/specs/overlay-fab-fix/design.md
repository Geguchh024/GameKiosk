# Overlay FAB Fix Bugfix Design

## Overview

This bugfix addresses two issues in the overlay FAB (floating action button) system:

1. **FAB Transparent Background**: The FAB window displays a visible rectangular background around the icon button, even though the window is configured as transparent. The fix involves CSS adjustments in `public/fab.html` to ensure only the icon pixels are visible.

2. **Controller Overlay Toggle**: The Home/Guide button (`Button::Mode` in gilrs) is unreliable for toggling the overlay because gilrs doesn't detect it on many controllers. The Start button should be repurposed to toggle the overlay instead of opening the edit modal. This requires changes in the Rust backend (`lib.rs`) and frontend (`App.tsx`, `useGamepad.ts`).

## Glossary

- **Bug_Condition (C)**: The conditions that trigger the bugs — (1) FAB window rendering with visible background, (2) Start button opening edit modal instead of toggling overlay
- **Property (P)**: The desired behavior — (1) FAB window fully transparent except icon pixels, (2) Start button toggles overlay open/close
- **Preservation**: Existing behaviors that must remain unchanged — mouse clicks on FAB, Ctrl+Shift+G shortcut, other gamepad buttons
- **FAB**: Floating Action Button — the small icon button shown during gameplay to access the overlay
- **Overlay**: The panel that appears when FAB is clicked or Start is pressed, showing game controls
- **gilrs**: Rust gamepad library used in the backend
- **Button::Mode**: The Home/Guide button in gilrs (unreliable detection)
- **Button::Start**: The Start button in gilrs (reliable detection)

## Bug Details

### Bug Condition

**Bug 1 - FAB Background**: The FAB window manifests a visible rectangular background when displayed, despite having `transparent: true` in the window configuration and `background: transparent` in CSS. This is likely due to missing `-webkit-background-color` or other platform-specific transparency properties.

**Bug 2 - Start Button Behavior**: The Start button currently triggers `onStart` in `useGamepad.ts`, which opens the edit modal in `App.tsx` instead of toggling the overlay. The Rust backend already handles `Button::Mode` for overlay toggle, but this button is unreliable in gilrs.

**Formal Specification:**
```
FUNCTION isBugCondition(input)
  INPUT: input of type { type: "fab_render" | "gamepad_button", button?: string }
  OUTPUT: boolean
  
  IF input.type == "fab_render" THEN
    RETURN fabWindowHasVisibleBackground()
  END IF
  
  IF input.type == "gamepad_button" AND input.button == "Start" THEN
    RETURN startButtonOpensEditModal() AND NOT startButtonTogglesOverlay()
  END IF
  
  RETURN false
END FUNCTION
```

### Examples

- **FAB Background Bug**: When FAB window is shown during gameplay, a semi-transparent or solid rectangular area is visible around the 48x48 icon, making it look like a floating rectangle instead of just an icon.
- **Start Button Bug**: User presses Start button while in the main launcher → edit modal opens instead of toggling overlay.
- **Start Button Bug**: User presses Start button during gameplay (FAB visible) → nothing happens or edit modal opens instead of showing overlay.
- **Edge Case**: User presses Start button when no game is selected → should be a no-op or toggle overlay if active.

## Expected Behavior

### Preservation Requirements

**Unchanged Behaviors:**
- Mouse clicks on the FAB icon must continue to invoke `show_overlay` and display the overlay panel
- Ctrl+Shift+G global shortcut must continue to toggle the overlay
- D-pad, A, B, X, Y, LB, RB, LT, RT buttons must continue to emit `gamepad-button` events and handle navigation/actions
- Close button on overlay panel must continue to hide overlay and show FAB
- Select button must continue to toggle favorites
- All other gamepad navigation (up/down/left/right) must work as before

**Scope:**
All inputs that do NOT involve FAB rendering or the Start button should be completely unaffected by this fix. This includes:
- Mouse interactions with FAB and overlay
- Keyboard shortcuts
- All other gamepad buttons
- Touch inputs (if applicable)

## Hypothesized Root Cause

Based on the bug description, the most likely issues are:

1. **FAB Background - Missing Platform-Specific CSS**: The `fab.html` may be missing `-webkit-background-color: transparent` or other WebKit-specific properties needed for true transparency on Windows/macOS. The `html` and `body` elements may need additional properties like `-webkit-app-region` adjustments.

2. **FAB Background - Window Configuration**: The Tauri window configuration may need additional transparency settings, though this is less likely since `transparent: true` is already set.

3. **Start Button - Wrong Handler**: The `onStart` callback in `useGamepad.ts` is wired to open the edit modal in `App.tsx`. This needs to be changed to either:
   - Remove `onStart` from frontend and let Rust handle Start button for overlay toggle
   - Or change `onStart` to invoke a Tauri command that toggles the overlay

4. **Start Button - Rust Backend**: The Rust `start_gamepad_thread` currently only handles `Button::Mode` for overlay toggle. It needs to also handle `Button::Start` the same way.

## Correctness Properties

Property 1: Bug Condition - FAB Transparency

_For any_ FAB window render where the window is displayed, the fixed CSS SHALL ensure only the icon pixels are visible with no background, border, or container rectangle — the window shall be fully transparent except for the icon image.

**Validates: Requirements 2.1**

Property 2: Bug Condition - Start Button Overlay Toggle

_For any_ Start button press on a controller (when FAB/overlay system is active), the fixed code SHALL toggle the overlay open/close, matching the intended Home button behavior.

**Validates: Requirements 2.2, 2.3**

Property 3: Preservation - Mouse and Keyboard Behavior

_For any_ input that is NOT a Start button press (mouse clicks, keyboard shortcuts, other gamepad buttons), the fixed code SHALL produce exactly the same behavior as the original code, preserving all existing functionality.

**Validates: Requirements 3.1, 3.2, 3.3, 3.4**

## Fix Implementation

### Changes Required

Assuming our root cause analysis is correct:

**File**: `public/fab.html`

**Specific Changes**:
1. **Add WebKit Transparency Properties**: Add `-webkit-background-color: transparent` to `html` and `body` elements
2. **Ensure No Default Backgrounds**: Add `background-color: transparent !important` to override any defaults
3. **Remove Potential Box Shadows**: Ensure no box-shadow or outline on any elements

**File**: `src-tauri/src/lib.rs`

**Function**: `start_gamepad_thread`

**Specific Changes**:
1. **Handle Button::Start for Overlay Toggle**: Add `Button::Start` to the same branch that handles `Button::Mode` for toggling the overlay
2. **Remove Start from gamepad-button Emission**: Remove `Button::Start => "Start"` from the button_name match so it doesn't emit to frontend

**File**: `src/App.tsx`

**Specific Changes**:
1. **Remove or Modify onStart Handler**: Remove the `onStart` callback from `useGamepad` call, or change it to a no-op since Rust will handle Start button for overlay toggle

**File**: `src/useGamepad.ts`

**Specific Changes**:
1. **Optional - Remove Start from Browser Polling**: If Start button should only be handled by Rust, remove it from `BTN_MAP` to avoid any frontend handling

## Testing Strategy

### Validation Approach

The testing strategy follows a two-phase approach: first, surface counterexamples that demonstrate the bugs on unfixed code, then verify the fixes work correctly and preserve existing behavior.

### Exploratory Bug Condition Checking

**Goal**: Surface counterexamples that demonstrate the bugs BEFORE implementing the fix. Confirm or refute the root cause analysis.

**Test Plan**: 
1. For FAB transparency: Inspect the FAB window visually and check computed styles
2. For Start button: Press Start button and observe if edit modal opens instead of overlay toggle

**Test Cases**:
1. **FAB Visual Test**: Launch a game, observe FAB window — verify visible background exists (will show bug on unfixed code)
2. **Start Button in Launcher**: Press Start in main launcher with a game selected — verify edit modal opens (will show bug on unfixed code)
3. **Start Button During Gameplay**: Press Start when FAB is visible — verify overlay does NOT toggle (will show bug on unfixed code)

**Expected Counterexamples**:
- FAB window shows rectangular background around icon
- Start button opens edit modal instead of toggling overlay
- Possible causes: missing CSS properties, wrong event handler wiring

### Fix Checking

**Goal**: Verify that for all inputs where the bug condition holds, the fixed function produces the expected behavior.

**Pseudocode:**
```
FOR ALL input WHERE isBugCondition(input) DO
  IF input.type == "fab_render" THEN
    result := renderFabWindow_fixed()
    ASSERT onlyIconPixelsVisible(result)
  END IF
  
  IF input.type == "gamepad_button" AND input.button == "Start" THEN
    result := handleStartButton_fixed()
    ASSERT overlayToggled(result)
  END IF
END FOR
```

### Preservation Checking

**Goal**: Verify that for all inputs where the bug condition does NOT hold, the fixed function produces the same result as the original function.

**Pseudocode:**
```
FOR ALL input WHERE NOT isBugCondition(input) DO
  ASSERT originalBehavior(input) = fixedBehavior(input)
END FOR
```

**Testing Approach**: Manual testing is recommended for these UI-focused bugs because:
- FAB transparency requires visual inspection
- Gamepad button behavior requires physical controller testing
- Automated tests would need complex UI automation setup

**Test Plan**: Observe behavior on UNFIXED code first, then verify after fix.

**Test Cases**:
1. **Mouse Click Preservation**: Verify clicking FAB icon continues to show overlay
2. **Keyboard Shortcut Preservation**: Verify Ctrl+Shift+G continues to toggle overlay
3. **Other Gamepad Buttons Preservation**: Verify D-pad, A, B, X, Y, LB, RB, LT, RT continue working
4. **Select Button Preservation**: Verify Select button continues to toggle favorites
5. **Overlay Close Preservation**: Verify close button on overlay continues to work

### Unit Tests

- Test CSS computed styles for FAB elements (background-color should be transparent)
- Test that Start button event is not emitted to frontend from Rust
- Test that overlay toggle logic works when Start is pressed

### Property-Based Tests

- Not applicable for these UI-focused bugs — manual testing is more appropriate

### Integration Tests

- Test full flow: launch game → FAB appears (transparent) → press Start → overlay shows → press Start → overlay hides
- Test that all other gamepad buttons continue to work during gameplay
- Test that mouse and keyboard interactions are unaffected

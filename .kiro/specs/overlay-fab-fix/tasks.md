# Implementation Plan

- [x] 1. Write bug condition exploration test
  - **Property 1: Bug Condition** - FAB Transparency and Start Button Behavior
  - **CRITICAL**: This test MUST FAIL on unfixed code - failure confirms the bugs exist
  - **DO NOT attempt to fix the test or the code when it fails**
  - **NOTE**: This test encodes the expected behavior - it will validate the fix when it passes after implementation
  - **GOAL**: Surface counterexamples that demonstrate the bugs exist
  - **Scoped PBT Approach**: For these UI bugs, scope to concrete failing cases:
    - FAB window displays with visible background around icon (CSS computed styles show non-transparent background)
    - Start button press triggers `onStart` which opens edit modal instead of toggling overlay
  - Test that FAB window CSS lacks WebKit transparency properties (from Bug Condition in design)
  - Test that Start button is mapped to `onStart` in useGamepad which opens edit modal (from Bug Condition in design)
  - Test that Rust backend emits "Start" to frontend instead of handling overlay toggle
  - Run test on UNFIXED code - expect FAILURE (this confirms the bugs exist)
  - Document counterexamples found:
    - `fab.html` missing `-webkit-background-color: transparent` on html/body
    - `useGamepad.ts` BTN_MAP includes `[9, "Start"]` which dispatches to `onStart`
    - `App.tsx` `onStart` callback opens edit modal via `openEditModal()`
    - `lib.rs` `Button::Start` maps to "Start" string and emits to frontend
  - Mark task complete when test is written, run, and failure is documented
  - _Requirements: 1.1, 1.2, 1.3_

- [x] 2. Write preservation property tests (BEFORE implementing fix)
  - **Property 2: Preservation** - Mouse, Keyboard, and Other Gamepad Behavior
  - **IMPORTANT**: Follow observation-first methodology
  - Observe behavior on UNFIXED code for non-buggy inputs:
    - Mouse click on FAB icon invokes `show_overlay` and displays overlay panel
    - Ctrl+Shift+G global shortcut toggles overlay
    - D-pad, A, B, X, Y, LB, RB, LT, RT buttons emit `gamepad-button` events and handle navigation
    - Select button toggles favorites via `onSelect` callback
    - Close button on overlay panel hides overlay and shows FAB
  - Write property-based tests capturing observed behavior patterns from Preservation Requirements:
    - For all non-Start gamepad buttons, `dispatch()` function routes to correct handler
    - For all mouse clicks on FAB, `show_overlay` Tauri command is invoked
    - For all Ctrl+Shift+G presses, overlay toggle logic executes
  - Verify tests pass on UNFIXED code
  - **EXPECTED OUTCOME**: Tests PASS (this confirms baseline behavior to preserve)
  - Mark task complete when tests are written, run, and passing on unfixed code
  - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [x] 3. Fix for FAB transparent background and Start button overlay toggle

  - [x] 3.1 Add WebKit transparency CSS to fab.html
    - Add `-webkit-background-color: transparent` to `html` and `body` elements
    - Add `background-color: transparent !important` to override any defaults
    - Ensure no box-shadow or outline on any elements that could cause visible background
    - _Bug_Condition: isBugCondition(input) where input.type == "fab_render" AND fabWindowHasVisibleBackground()_
    - _Expected_Behavior: onlyIconPixelsVisible(result) - FAB window fully transparent except icon pixels_
    - _Preservation: Mouse clicks on FAB icon continue to invoke show_overlay_
    - _Requirements: 2.1_

  - [x] 3.2 Handle Button::Start for overlay toggle in lib.rs
    - Add `Button::Start` to the same branch that handles `Button::Mode` for toggling overlay
    - Remove `Button::Start => "Start"` from the button_name match so it doesn't emit to frontend
    - The Start button should toggle overlay open/close using existing overlay toggle logic
    - _Bug_Condition: isBugCondition(input) where input.type == "gamepad_button" AND input.button == "Start"_
    - _Expected_Behavior: overlayToggled(result) - Start button toggles overlay open/close_
    - _Preservation: All other gamepad buttons continue to emit gamepad-button events_
    - _Requirements: 2.2, 2.3_

  - [x] 3.3 Remove or modify onStart handler in App.tsx
    - Remove the `onStart` callback from `useGamepad` call, or change it to a no-op
    - Since Rust will handle Start button for overlay toggle, frontend should not handle it
    - _Bug_Condition: startButtonOpensEditModal() AND NOT startButtonTogglesOverlay()_
    - _Expected_Behavior: Start button no longer opens edit modal in main launcher_
    - _Preservation: All other gamepad handlers (onConfirm, onBack, onSelect, etc.) unchanged_
    - _Requirements: 2.2, 2.3_

  - [x] 3.4 Verify bug condition exploration test now passes
    - **Property 1: Expected Behavior** - FAB Transparency and Start Button Overlay Toggle
    - **IMPORTANT**: Re-run the SAME test from task 1 - do NOT write a new test
    - The test from task 1 encodes the expected behavior
    - When this test passes, it confirms the expected behavior is satisfied:
      - FAB window renders only icon pixels with no visible background
      - Start button toggles overlay instead of opening edit modal
    - Run bug condition exploration test from step 1
    - **EXPECTED OUTCOME**: Test PASSES (confirms bugs are fixed)
    - _Requirements: 2.1, 2.2, 2.3_

  - [x] 3.5 Verify preservation tests still pass
    - **Property 2: Preservation** - Mouse, Keyboard, and Other Gamepad Behavior
    - **IMPORTANT**: Re-run the SAME tests from task 2 - do NOT write new tests
    - Run preservation property tests from step 2
    - **EXPECTED OUTCOME**: Tests PASS (confirms no regressions)
    - Confirm all tests still pass after fix:
      - Mouse clicks on FAB continue to show overlay
      - Ctrl+Shift+G continues to toggle overlay
      - D-pad, A, B, X, Y, LB, RB, LT, RT continue working
      - Select button continues to toggle favorites
      - Close button on overlay continues to work

- [x] 4. Checkpoint - Ensure all tests pass
  - Ensure all tests pass, ask the user if questions arise.
  - Verify FAB window displays only icon with no visible background
  - Verify Start button toggles overlay open/close
  - Verify all other gamepad buttons work as before
  - Verify mouse and keyboard interactions are unaffected

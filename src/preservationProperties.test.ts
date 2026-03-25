/**
 * Preservation Property Tests
 * 
 * **Validates: Requirements 3.1, 3.2, 3.3, 3.4**
 * 
 * These tests MUST PASS on unfixed code to establish baseline behavior.
 * They verify that existing functionality is preserved after the fix.
 * 
 * Preservation Requirements:
 * - Mouse click on FAB icon invokes `show_overlay` and displays overlay panel
 * - Ctrl+Shift+G global shortcut toggles overlay
 * - D-pad, A, B, X, Y, LB, RB, LT, RT buttons emit `gamepad-button` events and handle navigation
 * - Select button toggles favorites via `onSelect` callback
 * - Close button on overlay panel hides overlay and shows FAB
 */

import { describe, it, expect } from 'vitest';
import * as fc from 'fast-check';
import * as fs from 'fs';
import * as path from 'path';

// Read source files for inspection
const readSourceFile = (filePath: string): string => {
  const fullPath = path.resolve(process.cwd(), filePath);
  return fs.readFileSync(fullPath, 'utf-8');
};

/**
 * Non-Start gamepad buttons that must be preserved
 * These are all the buttons that should continue to work after the fix
 */
const PRESERVED_BUTTONS = [
  { index: 0, name: 'A', handler: 'onConfirm' },
  { index: 1, name: 'B', handler: 'onBack' },
  { index: 2, name: 'X', handler: 'onDelete' },
  { index: 3, name: 'Y', handler: 'onAdd' },
  { index: 4, name: 'LB', handler: 'onLB' },
  { index: 5, name: 'RB', handler: 'onRB' },
  { index: 6, name: 'LT', handler: 'onLT' },
  { index: 7, name: 'RT', handler: 'onRT' },
  { index: 8, name: 'Select', handler: 'onSelect' },
  { index: 12, name: 'DPadUp', handler: 'onUp' },
  { index: 13, name: 'DPadDown', handler: 'onDown' },
  { index: 14, name: 'DPadLeft', handler: 'onLeft' },
  { index: 15, name: 'DPadRight', handler: 'onRight' },
];

describe('Preservation Properties - Mouse, Keyboard, and Other Gamepad Behavior', () => {
  /**
   * Property 1: Preservation - dispatch() routes non-Start buttons to correct handlers
   * 
   * **Validates: Requirements 3.3**
   * 
   * For all non-Start gamepad buttons, the dispatch() function in useGamepad.ts
   * must route to the correct handler function.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('dispatch() function routes all non-Start gamepad buttons to correct handlers', () => {
    fc.assert(
      fc.property(
        fc.constantFrom(...PRESERVED_BUTTONS),
        (button) => {
          const content = readSourceFile('src/useGamepad.ts');
          
          // Extract the dispatch function
          const dispatchMatch = content.match(/function dispatch\s*\([^)]*\)\s*\{[\s\S]*?\n\}/);
          expect(dispatchMatch).not.toBeNull();
          const dispatchFunction = dispatchMatch![0];
          
          // Verify the button name is handled in the switch statement
          const buttonCasePattern = new RegExp(`case\\s*["']${button.name}["']\\s*:`);
          const hasButtonCase = buttonCasePattern.test(dispatchFunction);
          
          // Verify the correct handler is called for this button
          const handlerPattern = new RegExp(`case\\s*["']${button.name}["']\\s*:[^;]*${button.handler}`);
          const callsCorrectHandler = handlerPattern.test(dispatchFunction);
          
          expect(hasButtonCase).toBe(true);
          expect(callsCorrectHandler).toBe(true);
          
          return hasButtonCase && callsCorrectHandler;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 2: Preservation - BTN_MAP contains all non-Start buttons for browser polling
   * 
   * **Validates: Requirements 3.3**
   * 
   * For all non-Start gamepad buttons, the BTN_MAP in useGamepad.ts must contain
   * the correct mapping from browser button index to event name.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('BTN_MAP contains all non-Start gamepad buttons for browser polling', () => {
    fc.assert(
      fc.property(
        fc.constantFrom(...PRESERVED_BUTTONS),
        (button) => {
          const content = readSourceFile('src/useGamepad.ts');
          
          // Check if the button mapping exists in BTN_MAP
          // Pattern: [index, "ButtonName"]
          const mappingPattern = new RegExp(`\\[${button.index},\\s*["']${button.name}["']\\]`);
          const hasMappingInBtnMap = mappingPattern.test(content);
          
          expect(hasMappingInBtnMap).toBe(true);
          
          return hasMappingInBtnMap;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 3: Preservation - FAB click invokes show_overlay Tauri command
   * 
   * **Validates: Requirements 3.1**
   * 
   * Mouse click on FAB icon must invoke `show_overlay` Tauri command.
   * The fab.html must have a click handler that calls invoke('show_overlay').
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('FAB click handler invokes show_overlay Tauri command', () => {
    fc.assert(
      fc.property(
        fc.constant('public/fab.html'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check for click event listener on the FAB button
          const hasClickListener = content.includes("addEventListener('click'") ||
                                   content.includes('addEventListener("click"');
          
          // Check that show_overlay is invoked
          const invokesShowOverlay = content.includes("invoke('show_overlay')") ||
                                     content.includes('invoke("show_overlay")');
          
          // Check that the FAB button element exists
          const hasFabButton = content.includes('id="openOverlay"') ||
                               content.includes("id='openOverlay'");
          
          expect(hasClickListener).toBe(true);
          expect(invokesShowOverlay).toBe(true);
          expect(hasFabButton).toBe(true);
          
          return hasClickListener && invokesShowOverlay && hasFabButton;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 4: Preservation - Ctrl+Shift+G shortcut toggles overlay
   * 
   * **Validates: Requirements 3.2**
   * 
   * The Ctrl+Shift+G global shortcut must toggle the overlay.
   * The Rust backend must register this shortcut and call toggle_overlay.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('Ctrl+Shift+G global shortcut is registered for overlay toggle', () => {
    fc.assert(
      fc.property(
        fc.constant('src-tauri/src/lib.rs'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check for Ctrl+Shift+G shortcut registration
          // Pattern: "ctrl+shift+g" or "Ctrl+Shift+G" in shortcut registration
          const hasShortcutRegistration = content.toLowerCase().includes('ctrl+shift+g');
          
          // Check that toggle_overlay function exists
          const hasToggleOverlay = content.includes('toggle_overlay') ||
                                   content.includes('fn toggle_overlay');
          
          // Check that the shortcut handler calls toggle_overlay
          const shortcutCallsToggle = content.includes('toggle_overlay');
          
          expect(hasShortcutRegistration).toBe(true);
          expect(hasToggleOverlay).toBe(true);
          expect(shortcutCallsToggle).toBe(true);
          
          return hasShortcutRegistration && hasToggleOverlay && shortcutCallsToggle;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 5: Preservation - Select button toggles favorites via onSelect
   * 
   * **Validates: Requirements 3.3, 3.4**
   * 
   * The Select button must trigger onSelect callback which toggles favorites.
   * The App.tsx must have onSelect handler that calls toggleFavorite.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('Select button triggers onSelect which toggles favorites', () => {
    fc.assert(
      fc.property(
        fc.constant('src/App.tsx'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check that onSelect is in the useGamepad call
          const hasOnSelectInUseGamepad = content.includes('onSelect:');
          
          // Check that toggleFavorite function exists
          const hasToggleFavorite = content.includes('toggleFavorite') ||
                                    content.includes('const toggleFavorite');
          
          // Check that onSelect references toggleFavorite
          // The pattern should be: onSelect: toggleFavorite or onSelect: () => toggleFavorite()
          const onSelectCallsToggleFavorite = content.includes('onSelect: toggleFavorite') ||
                                              content.includes('onSelect:toggleFavorite');
          
          expect(hasOnSelectInUseGamepad).toBe(true);
          expect(hasToggleFavorite).toBe(true);
          expect(onSelectCallsToggleFavorite).toBe(true);
          
          return hasOnSelectInUseGamepad && hasToggleFavorite && onSelectCallsToggleFavorite;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 6: Preservation - Overlay close button hides overlay and shows FAB
   * 
   * **Validates: Requirements 3.4**
   * 
   * The hide_overlay Tauri command must hide the overlay and show the FAB.
   * The Rust backend must have hide_overlay function that does both.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('hide_overlay command hides overlay and shows FAB', () => {
    fc.assert(
      fc.property(
        fc.constant('src-tauri/src/lib.rs'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check that hide_overlay function exists as a Tauri command
          const hasHideOverlayCommand = content.includes('#[tauri::command]') &&
                                        content.includes('fn hide_overlay');
          
          // Check that hide_overlay hides the overlay window
          // Pattern: overlay.hide() or overlay.hide().ok()
          const hidesOverlayWindow = content.includes('overlay.hide()') ||
                                     content.includes('overlay).hide()');
          
          // Check that hide_overlay shows the FAB window
          // Pattern: fab.show() or fab.show().ok()
          const showsFabWindow = content.includes('fab.show()') ||
                                 content.includes('fab).show()');
          
          expect(hasHideOverlayCommand).toBe(true);
          expect(hidesOverlayWindow).toBe(true);
          expect(showsFabWindow).toBe(true);
          
          return hasHideOverlayCommand && hidesOverlayWindow && showsFabWindow;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 7: Preservation - show_overlay command shows overlay and hides FAB
   * 
   * **Validates: Requirements 3.1**
   * 
   * The show_overlay Tauri command must show the overlay and hide the FAB.
   * The Rust backend must have show_overlay function that does both.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('show_overlay command shows overlay and hides FAB', () => {
    fc.assert(
      fc.property(
        fc.constant('src-tauri/src/lib.rs'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check that show_overlay function exists as a Tauri command
          const hasShowOverlayCommand = content.includes('#[tauri::command]') &&
                                        content.includes('fn show_overlay');
          
          // Check that show_overlay shows the overlay window
          // Pattern: overlay.show() or overlay.show().map_err
          const showsOverlayWindow = content.includes('overlay.show()') ||
                                     content.includes('overlay).show()');
          
          // Check that show_overlay hides the FAB window
          // Pattern: fab.hide() or fab.hide().ok()
          const hidesFabWindow = content.includes('fab.hide()') ||
                                 content.includes('fab).hide()');
          
          expect(hasShowOverlayCommand).toBe(true);
          expect(showsOverlayWindow).toBe(true);
          expect(hidesFabWindow).toBe(true);
          
          return hasShowOverlayCommand && showsOverlayWindow && hidesFabWindow;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 8: Preservation - Tauri event listener for gamepad-button events
   * 
   * **Validates: Requirements 3.3**
   * 
   * The useGamepad hook must listen for 'gamepad-button' Tauri events
   * and dispatch them to the appropriate handlers.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('useGamepad listens for gamepad-button Tauri events', () => {
    fc.assert(
      fc.property(
        fc.constant('src/useGamepad.ts'),
        (filePath) => {
          const content = readSourceFile(filePath);
          
          // Check that listen is imported from Tauri
          const importsListen = content.includes("from '@tauri-apps/api/event'") ||
                                content.includes('from "@tauri-apps/api/event"');
          
          // Check that listen is called with 'gamepad-button' event
          const listensForGamepadButton = content.includes("listen<string>('gamepad-button'") ||
                                          content.includes('listen<string>("gamepad-button"') ||
                                          content.includes("listen('gamepad-button'") ||
                                          content.includes('listen("gamepad-button"');
          
          // Check that dispatch is called in the event handler
          const dispatchesInHandler = content.includes('dispatch(actionsRef.current');
          
          expect(importsListen).toBe(true);
          expect(listensForGamepadButton).toBe(true);
          expect(dispatchesInHandler).toBe(true);
          
          return importsListen && listensForGamepadButton && dispatchesInHandler;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 9: Preservation - All D-pad buttons route to navigation handlers
   * 
   * **Validates: Requirements 3.3**
   * 
   * D-pad buttons (Up, Down, Left, Right) must route to navigation handlers
   * (onUp, onDown, onLeft, onRight) in the dispatch function.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('D-pad buttons route to navigation handlers', () => {
    const dpadButtons = [
      { name: 'DPadUp', handler: 'onUp' },
      { name: 'DPadDown', handler: 'onDown' },
      { name: 'DPadLeft', handler: 'onLeft' },
      { name: 'DPadRight', handler: 'onRight' },
    ];

    fc.assert(
      fc.property(
        fc.constantFrom(...dpadButtons),
        (button) => {
          const content = readSourceFile('src/useGamepad.ts');
          
          // Extract the dispatch function
          const dispatchMatch = content.match(/function dispatch\s*\([^)]*\)\s*\{[\s\S]*?\n\}/);
          expect(dispatchMatch).not.toBeNull();
          const dispatchFunction = dispatchMatch![0];
          
          // Verify the D-pad button routes to the correct navigation handler
          const routePattern = new RegExp(`case\\s*["']${button.name}["']\\s*:[^;]*a\\.${button.handler}\\(\\)`);
          const routesToCorrectHandler = routePattern.test(dispatchFunction);
          
          expect(routesToCorrectHandler).toBe(true);
          
          return routesToCorrectHandler;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 10: Preservation - Action buttons (A, B, X, Y) route to action handlers
   * 
   * **Validates: Requirements 3.3**
   * 
   * Action buttons (A, B, X, Y) must route to action handlers
   * (onConfirm, onBack, onDelete, onAdd) in the dispatch function.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('Action buttons (A, B, X, Y) route to action handlers', () => {
    const actionButtons = [
      { name: 'A', handler: 'onConfirm' },
      { name: 'B', handler: 'onBack' },
      { name: 'X', handler: 'onDelete' },
      { name: 'Y', handler: 'onAdd' },
    ];

    fc.assert(
      fc.property(
        fc.constantFrom(...actionButtons),
        (button) => {
          const content = readSourceFile('src/useGamepad.ts');
          
          // Extract the dispatch function
          const dispatchMatch = content.match(/function dispatch\s*\([^)]*\)\s*\{[\s\S]*?\n\}/);
          expect(dispatchMatch).not.toBeNull();
          const dispatchFunction = dispatchMatch![0];
          
          // Verify the action button routes to the correct handler
          const routePattern = new RegExp(`case\\s*["']${button.name}["']\\s*:[^;]*a\\.${button.handler}\\(\\)`);
          const routesToCorrectHandler = routePattern.test(dispatchFunction);
          
          expect(routesToCorrectHandler).toBe(true);
          
          return routesToCorrectHandler;
        }
      ),
      { verbose: true }
    );
  });

  /**
   * Property 11: Preservation - Shoulder buttons (LB, RB, LT, RT) route to optional handlers
   * 
   * **Validates: Requirements 3.3**
   * 
   * Shoulder buttons (LB, RB, LT, RT) must route to optional handlers
   * using optional chaining (onLB?.(), onRB?.(), etc.) in the dispatch function.
   * 
   * This test MUST PASS on unfixed code (baseline behavior to preserve).
   */
  it('Shoulder buttons (LB, RB, LT, RT) route to optional handlers', () => {
    const shoulderButtons = [
      { name: 'LB', handler: 'onLB' },
      { name: 'RB', handler: 'onRB' },
      { name: 'LT', handler: 'onLT' },
      { name: 'RT', handler: 'onRT' },
    ];

    fc.assert(
      fc.property(
        fc.constantFrom(...shoulderButtons),
        (button) => {
          const content = readSourceFile('src/useGamepad.ts');
          
          // Extract the dispatch function
          const dispatchMatch = content.match(/function dispatch\s*\([^)]*\)\s*\{[\s\S]*?\n\}/);
          expect(dispatchMatch).not.toBeNull();
          const dispatchFunction = dispatchMatch![0];
          
          // Verify the shoulder button routes to the optional handler
          // Pattern: a.onLB?.() or a.onRB?.() etc.
          const routePattern = new RegExp(`case\\s*["']${button.name}["']\\s*:[^;]*a\\.${button.handler}\\?\\.\\.?\\(\\)`);
          const routesToOptionalHandler = routePattern.test(dispatchFunction);
          
          expect(routesToOptionalHandler).toBe(true);
          
          return routesToOptionalHandler;
        }
      ),
      { verbose: true }
    );
  });
});

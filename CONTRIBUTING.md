# Contributing to GameKiosk

Thank you for your interest in contributing to GameKiosk!

## Getting Started

1. Fork the repository
2. Clone your fork: `git clone https://github.com/yourusername/GameKiosk.git`
3. Install dependencies: `npm install`
4. Run in dev mode: `npm run tauri dev`

## Development Setup

### Prerequisites

- Node.js 18+
- Rust (latest stable)
- Windows 10/11 (primary target platform)

### Project Structure

```
GameKiosk/
├── src/                 # React frontend
│   ├── App.tsx          # Main application component
│   ├── styles.css       # Global styles
│   └── ...
├── src-tauri/           # Rust backend
│   ├── src/
│   │   ├── lib.rs       # Main Tauri commands
│   │   └── main.rs      # Entry point
│   └── tauri.conf.json  # Tauri configuration
├── public/              # Static assets
│   ├── fab.html         # Floating action button
│   └── overlay.html     # Overlay menu
└── ...
```

## Code Style

- **TypeScript/React**: Use functional components with hooks
- **Rust**: Follow standard Rust conventions, run `cargo fmt`
- **CSS**: Use CSS variables for theming

## Pull Request Process

1. Create a feature branch from `main`
2. Make your changes
3. Test thoroughly with both keyboard and controller
4. Update documentation if needed
5. Submit a PR with a clear description

## Reporting Issues

When reporting bugs, please include:

- Windows version
- Steps to reproduce
- Expected vs actual behavior
- Screenshots if applicable
- Controller model (if gamepad-related)

## Feature Requests

Feature requests are welcome! Please describe:

- The problem you're trying to solve
- Your proposed solution
- Any alternatives you've considered

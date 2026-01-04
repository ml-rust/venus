# API Stability Policy

This document defines the stability guarantees for Venus APIs and versioning policy.

## Overview

Venus follows **Semantic Versioning (SemVer)** for version numbers: `MAJOR.MINOR.PATCH`

**Before 1.0.0:**

- Venus is in active development (0.x.y versions)
- Breaking changes may occur in minor versions (0.x.0)
- We will document all breaking changes in release notes

**After 1.0.0:**

- Breaking changes only in major versions (1.0 → 2.0)
- New features in minor versions (1.0 → 1.1)
- Bug fixes in patch versions (1.0.0 → 1.0.1)

## API Tiers

### ✅ Tier 1: Stable APIs

**Crate**: `venus`

These APIs are intended for end users writing notebooks and are considered **stable**:

- `#[venus::cell]` proc macro
- `venus::prelude::*` module
- `Render` trait
- Widget functions:
  - `input_slider`, `input_slider_labeled`, `input_slider_with_step`
  - `input_text`, `input_text_labeled`, `input_text_with_default`
  - `input_checkbox`, `input_checkbox_labeled`
  - `input_select`, `input_select_labeled`

**Guarantees** (starting from 0.1.0):

- ✅ **SemVer compliance**: Breaking changes only in major versions (before 1.0: may occur in minor versions but will be documented)
- ✅ **Deprecation policy**: Minimum 1 minor version deprecation warning before removal
- ✅ **Migration guides**: Clear upgrade paths for breaking changes
- ✅ **Stability across patch versions**: Bug fixes only, no API changes

**Examples of breaking changes** (require major version bump after 1.0):

- Changing `#[venus::cell]` syntax or semantics
- Removing or renaming items from `venus::prelude`
- Changing `Render` trait signatures
- Removing widget functions

### ⚠️ Tier 2: Internal APIs (Unstable)

**Crate**: `venus-core`, `venus-server`, `venus-sync`

These APIs are internal implementation details and are **UNSTABLE**:

- Graph engine (`venus_core::graph`)
- Compilation pipeline (`venus_core::compile`)
- State management (`venus_core::state`)
- Execution engine (`venus_core::execute`)
- IPC protocol (`venus_core::ipc`)
- Server protocol (`venus_server::protocol`)
- CLI commands and flags

**Guarantees**:

- ❌ **No SemVer guarantees**: Breaking changes may occur in any version (0.x.y or even patches if critical)
- ❌ **No deprecation warnings**: APIs may be removed or changed without warning
- ❌ **No migration guides**: Internal refactoring may require code updates

**Why unstable?**
These APIs are for:

- Building custom notebook tools and extensions
- Advanced integrations with Venus internals
- Contributing to Venus development

We reserve the right to refactor, optimize, and improve these internal systems without
backward compatibility concerns.

**If you need stable internal APIs**, please open an issue describing your use case.
We may stabilize specific APIs based on community needs.

## Deprecation Policy

### For Stable APIs (Tier 1)

When we need to remove or change a stable API:

1. **Deprecation announcement** (minor version N):

   - Add `#[deprecated]` attribute with message
   - Document alternative in deprecation message
   - Add migration guide to release notes

2. **Deprecation period** (minimum 1 minor version):

   - Users get compiler warnings
   - Old API still works
   - Time to migrate code

3. **Removal** (major version N+1):
   - API removed in next major version
   - Migration guide available

**Example**:

```rust
// Version 0.5.0: Add new API
pub fn input_slider_v2(min: f64, max: f64, step: f64) -> f64 { ... }

// Version 0.6.0: Deprecate old API
#[deprecated(since = "0.6.0", note = "Use input_slider_v2 instead")]
pub fn input_slider(min: f64, max: f64) -> f64 { ... }

// Version 1.0.0: Remove old API
// input_slider() no longer exists
```

### For Unstable APIs (Tier 2)

No deprecation warnings. Internal APIs may change or be removed immediately.

## Breaking Changes

### What constitutes a breaking change?

**For stable APIs (Tier 1)**:

- Removing or renaming public items
- Changing function signatures
- Changing trait definitions
- Changing macro syntax or semantics
- Changing serialization formats (for persisted data)

**Not breaking**:

- Adding new public items
- Deprecating (but not removing) APIs
- Adding new optional trait methods with defaults
- Internal implementation changes (performance, bug fixes)
- Documentation changes

### How breaking changes are communicated

1. **Release notes**: All breaking changes listed with migration guide
2. **CHANGELOG.md**: Updated with each release
3. **Compiler errors**: For Tier 1 APIs, code won't compile with clear error messages
4. **Migration guide**: Step-by-step instructions for upgrading

## Version Support

### Current version: 0.x (Active Development)

- **Active development**: New features, breaking changes allowed
- **Bug fixes**: Patch releases for critical bugs
- **No LTS**: Only latest 0.x version supported

### After 1.0 release

- **Latest minor**: Active development (new features, bug fixes)
- **Previous minor**: Bug fixes for 6 months
- **Older versions**: No support (upgrade recommended)

**Example**:

- 1.2.x: Active development
- 1.1.x: Bug fixes for 6 months after 1.2.0 release
- 1.0.x: No support (upgrade to 1.1 or 1.2)

## Pre-release Versions

- **Alpha** (`0.1.0-alpha.1`): Unstable, breaking changes expected
- **Beta** (`0.1.0-beta.1`): Feature-complete, API may still change
- **RC** (`0.1.0-rc.1`): Release candidate, API frozen, bug fixes only

## Stability Exceptions

Breaking changes may occur in patch versions (even for stable APIs) if:

1. **Security vulnerability**: Critical security fix requires API change
2. **Soundness bug**: Prevents unsafe code or undefined behavior
3. **Critical correctness bug**: Major functionality broken

These will be:

- Clearly documented in release notes
- Marked with `BREAKING:` prefix
- Include migration instructions

## File Format Stability

### Notebook Files (`.rs`)

**Stable**: Notebook source files use standard Rust syntax. No breaking changes.

### Cache Files (`.venus/` directory)

**Unstable**: Internal cache format may change without notice. Venus will automatically
invalidate and rebuild caches when format changes.

### Export Formats

- **HTML export** (`.html`): Stable structure, may add new features
- **Jupyter export** (`.ipynb`): Follows Jupyter notebook format v4.x (stable)

## Requesting Stabilization

If you need stable APIs for use cases beyond basic notebook development:

1. **Open an issue**: Describe your use case and which APIs you need
2. **Community discussion**: We'll evaluate if the API is ready for stabilization
3. **API review**: Ensure the API is well-designed and sustainable
4. **Stabilization**: Move API to Tier 1 with stability guarantees

## Summary

| Aspect               | Tier 1 (Stable)                | Tier 2 (Unstable)                  |
| -------------------- | ------------------------------ | ---------------------------------- |
| **Crates**           | `venus`                        | `venus-core`, `venus-server`, etc. |
| **SemVer**           | ✅ Yes (after 1.0)             | ❌ No                              |
| **Deprecation**      | ✅ Minimum 1 version           | ❌ None                            |
| **Breaking changes** | Major version only (after 1.0) | Any version                        |
| **Target users**     | Notebook authors               | Tool builders, contributors        |

**For most users**: Use the `venus` crate APIs. They are stable and won't break your notebooks.

**For advanced users**: Internal APIs (`venus-core`, etc.) are unstable. Expect changes.

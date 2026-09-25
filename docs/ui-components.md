# Designing Delight's UI components

Notes for building components in `crates/ui` (`delight-ui`, host-only). Distilled
from reading [`gpui-component`](https://github.com/longbridge/gpui-kit) — 0.5.1,
built on the same `gpui 0.2.2` we use, and 0.6.6 / `gpui-base` (on `gpui-pre`) —
plus GPUI's own `examples/input.rs`. File references are to `gpui-component-0.5.1/src`
unless marked 0.6.

The kit's visual design (macOS-native, dense: 26pt controls, 12–13px text) is ours;
these notes are about **structure**.

## 1. Two kinds of component

| Kind | Shape | Use when |
|---|---|---|
| **Stateless** | builder struct + `#[derive(IntoElement)]` + `RenderOnce` | Everything that's fully described by its inputs each frame: button, switch, segmented control, keycap, badge, icon, group. Rebuilt every frame, consumed by `render(self, ..)`. |
| **Stateful** | `Entity<XxxState>` + `Render` + `Focusable` + `EventEmitter` | State that must outlive a frame and that the component mutates itself: the text editor. |

- **Controlled, not self-managing.** Value controls take the current value and
  report the *new* one: `Switch::new(id).checked(on).on_change(|&on, window, cx| ..)`
  (their `switch.rs:20, 209`). The caller owns the state.
- **Tiny per-element state without an Entity:** `window.use_keyed_state(id, cx, |_, cx| ..)`
  — e.g. a stateless button's focus handle (`button/button.rs:436`). This is why
  interactive stateless components take an `ElementId`.
- **State + thin view wrapper** for stateful ones: the app owns `Entity<EditorState>`
  and renders `Input::new(&state).placeholder(..)`; the wrapper adds per-site
  appearance and routes actions into the entity with `window.listener_for(&state, ..)`
  (`input/input.rs:142`).

## 2. A component, end to end

```rust
#[derive(IntoElement)]
pub struct Button {
    id: ElementId,
    label: SharedString,
    icon: Option<Icon>,
    variant: ButtonVariant,
    size: Size,
    disabled: bool,
    on_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App)>>,
}

impl Button {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self { .. }
    pub fn icon(mut self, icon: impl Into<Icon>) -> Self { .. }
    pub fn on_click(mut self, f: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static) -> Self { .. }
}

impl Disableable for Button { .. }
impl Sizable for Button { .. }

impl RenderOnce for Button {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let t = cx.theme();                         // &Theme from a Global — no argument threading
        let c = self.variant.colors(t);             // one colour table per component
        div().id(self.id)
            .h(self.size.control_height())          // sizes from one table, not literals
            .bg(c.bg).text_color(c.fg)
            .when(!self.disabled, |d| d.hover(|s| s.bg(c.hover)).active(|s| s.bg(c.active)))
            .when(self.disabled, |d| d.opacity(0.5).on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation()))
            .when_some(self.on_click.filter(|_| !self.disabled), |d, f| d.on_click(move |e, w, cx| f(e, w, cx)))
            ..
    }
}
```

Rules this encodes:

- **Constructor takes the id first**, everything else is a named builder — no positional
  `Option` arguments.
- **Read the theme inside `render`** via `cx.theme()`; never pass `&Theme` into every call.
- **Callbacks are `Rc<dyn Fn>`** (cheap to clone into several listeners), signature
  `Fn(&Event, &mut Window, &mut App)`; value controls pass the new value (`&bool`, `&usize`).
- **Disabled is three layers:** no hover/active styles, `stop_propagation` on mouse-down
  so parents don't react, and `on_click` not attached. Disabled controls also don't
  track focus (out of the tab order).
- **Keyboard activation is free:** in `gpui 0.2.2` a focused element with `on_click`
  fires on Enter/Space (`ClickEvent::Keyboard`). Tracking focus is enough.
- **Hover *and* active** (`.hover(..)`, `.active(..)`); focus drawn as a ring.

## 3. Shared traits (`styled.rs` in our kit)

- `Sizable` — required `with_size(Size)`, provided `.small()` / `.large()`.
- `Disableable` — `disabled(bool)`. Implement it on **every** control (their `Input`
  forgot and has an inherent method — inconsistent, `input/input.rs:130`).
- `Selectable` — `selected(bool)` where relevant.
- `StyledExt` — blanket `impl<E: Styled>`: `h_flex()`, `v_flex()`, `surface()`
  (a reusable bg + border + radius + shadow recipe), `focus_ring(..)`.
- **One size table.** `enum Size { Small, Medium, Large }` mapped in one place
  (control height, icon size, text size). Ours: Small 20 / Medium 26 / Large 32 control
  heights, matching today's 26pt default. Avoid clever size arithmetic (their
  `Size::max`/`min` are inverted relative to the names, `styled.rs:312`).

## 4. Theme

- **Store the *resolved* theme in a GPUI `Global`**, read with an extension trait:
  ```rust
  pub trait ActiveTheme { fn theme(&self) -> &Theme; }
  impl ActiveTheme for App { fn theme(&self) -> &Theme { cx.global::<Theme>() } }
  ```
  `Context<V>` derefs to `App`, so `cx.theme()` works in `Render` and `RenderOnce`.
  Returns `&Theme` — **no cloning or rebuilding per call** (their 0.6 base clones on
  every read; our old kit rebuilt the whole palette on every `theme()` call).
- **Recompute only on change**: when the user switches Light/Dark/System, and when
  the macOS appearance changes (observe window appearance), then `cx.refresh_windows()`.
- **Role tokens, not component tokens.** 0.5 had 111 flat colours including
  `accordion_hover`, `switch_thumb`; 0.6 explicitly reversed that. Our
  `delight_sdk::Theme` (colours / status / palette / syntax / text / metrics) already
  follows the rule — keep it that way; derive hover/pressed shades from role tokens.
- **One source of truth.** 0.6 keeps two theme globals and needs a manual `sync_base`
  (a documented footgun). We have one.
- **Inherit from a root**: set `font_family`, `text_color`, `text_size` once at the
  window root so leaves inherit instead of re-setting them.
- **Colour per variant as a table**, one `fn colors(variant, &Theme) -> Colors { bg, fg,
  hover, active, border }`. Their button re-matches 10 variants in 5 functions
  (~360 lines, `button/button.rs:623-984`) and shipped a leftover red hover (`:518`).

## 5. Keyboard and focus

- **Per-module `init(cx)`** binds keys under a **key context** (`const CONTEXT: &str =
  "Editor"`); one crate-level `ui::init(cx)` calls theme init first, then each module's.
  Stateless components need no init.
- **Actions** via `actions!(namespace, [VerbObject, ..])`. Shared cross-component actions
  (`Confirm`, `Cancel`, `SelectPrev`, `SelectNext`) live in one `actions.rs` and are
  bound per context.
- **Propagation by omission:** register a handler only where it applies — e.g. a
  single-line editor doesn't handle ↑/↓ at all, so they reach the launcher list
  (their `input/input.rs:170-179`). At an edge of a multi-line editor, call
  `cx.propagate()`.
- **Focus handles:** stateful components create one in `new` (`cx.focus_handle()`) and
  implement `Focusable`; stateless ones use keyed state. Tab order via
  `FocusHandle::tab_index(..)` / `tab_stop(..)`; `window.focus_next()/focus_prev()`.
- **Handler wiring** is a wall of `.on_action(..)` lines; a small local macro keeps it
  readable and in one place.

## 6. The text editor

GPUI has no text input; Zed's own fields use its full code editor, which isn't
reusable. GPUI publishes `examples/input.rs` (746 lines) as the reference pattern:
actions + key bindings, `EntityInputHandler` for IME, and a custom `Element` that
shapes and paints. Our editor follows it.

**Structure (by concern, not by feature flag):**

| File | Holds |
|---|---|
| `text.rs` | pure navigation on `&str`: grapheme / word / line boundaries, clamping, UTF-16 — unit-tested |
| `history.rs` | undo/redo |
| `blink.rs` | cursor blinking |
| `keymap.rs` | actions + key bindings (+ the handler-wiring macro) |
| `state.rs` | the editor entity: content, selection, editing commands, events |
| `ime.rs` | `EntityInputHandler` |
| `element.rs` | layout, selection/cursor/placeholder painting |

**Decisions:**

- **`String`, not a rope.** Launcher input and settings fields are small. (They use
  `ropey`; worth it only for documents.)
- **Undo stores diffs, not snapshots**: `Edit { start, old, new, selection_before }`.
  Coalesce adjacent edits of the same *intent* (typing, backspace, delete-forward) into
  one step, as 0.6's `UndoManager` does — typing "hello" undoes in one go. **A new edit
  clears redo.** Bound by steps and bytes. (0.5's `History` never cleared redo, grouped
  by time — 1 s — and popped in an O(n²) loop: all three are bugs to avoid.)
- **IME composition** is one undo step (bracket it) and never recorded letter by letter.
- **Blinking cursor** is a tiny entity (their `input/blink_cursor.rs`, 92 lines):
  toggles `visible` every 500 ms via a spawned timer, uses an `epoch` counter so stale
  timers stop, `pause()` on every keystroke keeps it solid while typing. Paint the cursor
  only when focused, visible and the window is active.
- **Events** say what happened: `Change`, `Submit`, `Focus`, `Blur` — not just `Changed`.
- **Offsets are UTF-8 bytes internally**; convert to UTF-16 only at the IME boundary.
- **Never trust offsets** from the platform or the mouse: clamp to char boundaries.
- **Large text** (only if it ever matters): re-wrap only touched lines, shape only the
  visible range. Not needed for a launcher.
- **Skip:** LSP, search, code-editor mode, format masks, number/OTP inputs — and keep the
  state small. Their `InputState` has 40+ fields and grows to 9.4k lines in 0.6.

## 7. Icons

- An `IconName` enum generated by the same macro that embeds the SVGs, so a name can't
  point at a missing file. `Icon::new(IconName::Copy)` is `RenderOnce`, `Sizable`, and
  **inherits the current text colour and size when not set** — an icon in a button
  matches its label automatically. Components take `impl Into<Icon>`.

## 8. Tooltips

- GPUI's `.tooltip(|window, cx| AnyView)` on a stateful element. A tooltip that names
  an action looks its shortcut up at show time
  (`window.highest_precedence_binding_for_action_in_context`), so the hint can never go
  stale.

## 9. Organisation

- **One crate.** 0.6's split into unstyled `gpui-base` + styled façade exists so third
  parties can restyle behaviour; it costs duplicated fields, two theme globals and escape
  hatches. Not our situation.
- `lib.rs`: private infrastructure modules, `pub mod` per component, glob re-exports of
  the cross-cutting pieces (theme, `styled` traits, icons) — the root *is* the prelude —
  and `pub fn init(cx)`.
- Names: `Xxx` (component), `XxxState` (entity), `XxxEvent`; `Xxx::new(id)`; key context
  `const CONTEXT`.
- **Don't require a specific window root view** (their `Root::read` panics otherwise).
- **Don't spawn tasks inside `render`** (their `Switch` does, `switch.rs:158`).
- **A component implements `RenderOnce` *or* `Render`, not both** — their `Icon` has
  two diverged bodies.

## 10. Testing

- Pure logic (text navigation, history, masks, palette completeness) as plain `#[test]`s.
- Behaviour with `#[gpui::test]` harnesses — all available in `gpui 0.2.2`:
  `cx.add_window_view(..)`, `cx.simulate_click(..)`, `cx.simulate_keystrokes("enter")`,
  counters in `Rc<Cell<usize>>`. Worth covering: disabled controls neither fire nor
  bubble, Enter/Space activate, segmented/switch report the new value, editor undo
  coalescing, IME marked text, ↑/↓ propagation at the edges.

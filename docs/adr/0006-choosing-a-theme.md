# Choosing a theme

The app follows the OS. It now also lets the user overrule it — Light, Dark, or System — from a control in the sidebar footer, beside Usage.

The choice is stored in `localStorage` under `devcode:theme`, and **System is stored as the absence of a key** rather than as the string `"system"`. That way a later change to what System resolves to reaches people who never touched the control, which is the whole point of their not having touched it. Nothing about the theme goes through the Rust side: it is a preference about how this window looks, it has no bearing on agents or worktrees, and giving it a place in the persisted app state would have meant an IPC round trip before the app could decide what colour to be.

The resolution happens **in script, not in CSS**. `theme.svelte.ts` reduces the three-way preference to the one value the stylesheet cares about and writes it to `<html data-theme>`; `theme.css` then has exactly one light block and one dark block. The alternative — keeping `@media (prefers-color-scheme: dark)` and layering `[data-theme]` overrides on top of it — needs every dark value written twice, once inside the media query and once outside, and gets the precedence wrong in one direction or the other unless both are complete. Resolving first is what makes an override possible at all without that duplication.

Because CSS can no longer work it out alone, an **inline script in `app.html`** applies the stored value before the first paint, so the window never flashes white on launch. It duplicates three lines of `theme.svelte.ts` deliberately: waiting on the module means waiting on the bundle, which is exactly the flash it exists to prevent. The two must agree on the storage key and the attribute, and both say so.

The store also writes **`color-scheme`** alongside the attribute. That is the part of theming the stylesheet cannot reach — native form controls, scrollbars where the engine draws its own, and the canvas behind the document all read it, and without it a dark window shows a white flash on overscroll.

While the preference is System, a `matchMedia` listener keeps it following the OS live, so a machine that switches at sunset switches the app with it rather than at the next launch.

The control is **one button that cycles**, not three. The choice is small and made rarely, the icon shows what you are looking at while the label says where the value came from (`System (dark)` rather than just `System`), and a segmented control would cost the rail more width than it has. It is a `.btn .btn-ghost`, the same quiet full-width footer row as Usage beside it.

Considered and rejected: **persisting through the Rust side**, covered above. **A settings window** to hold this one preference, which is where a third and fourth preference would push it, but not where one belongs. **Skipping the pre-paint script** and accepting a flash, which is cheap to avoid and looks broken. **A `"system"` value written to storage**, which freezes today's meaning of the default into every existing install.

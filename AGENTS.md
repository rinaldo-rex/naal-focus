## Git

- We're using git alongside with JJ. 
- Use atomic commits (or JJ descriptions) whenever you're making changes - Make the changes isolatable atomic pieces of work. 

## Desktop-only scope

- Naal Focus is a desktop-only application. Design, implement, and verify features for desktop use, including multiple monitors.
- Do not spend project work on mobile layouts, mobile-specific behavior, or mobile viewport testing unless explicitly requested by the user.
- Continue supporting different desktop window sizes, monitor resolutions, aspect ratios, and display scaling; desktop-only does not mean one fixed screen size.

# Generic

- When discussing options with the user - Give a brief, and use an example wherever possible to show them the tradeoffs.
- Install updates with `just install` during development when a feature is completed and confirmed by the user. For reloading the applet after installing please ask the user and don't bother trying to do it yourself unless explicitly asked by the user. 

## Documentation

- Treat docs/html/index.html as the go-to source of truth for app behavior and UI intent. Read it before changes, and update it and relevant screenshots
   alongside implementation.
- Don't add documentation where code is self-explanatory. Rule of thumb: Your code should be clean enough to self-explain what it is doing, and the code comments (if required) should explain _why_ you're doing it in certain ways. The HTML documentation is user-facing, so keep it brief, and explain the feature in layman terms only. 

### Generic UI/UX guidelines

- Cut taglines, helper text and secondary labels. If a screen needs them to make sense, fix the layout.
- Make the best decision that will elevate all three: the user experience (UX), developer experience (DX), and agent experience (AX) always, without breaking anything.

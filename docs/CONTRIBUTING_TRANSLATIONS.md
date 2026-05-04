# Contributing Translations

iDescriptor uses Qt's translation system (Qt Linguist) to provide a localised
user interface. Translations live in `translations/idescriptor_<locale>.ts`
and are compiled into binary `.qm` files at build time and embedded as Qt
resources under `:/i18n/`.

## Adding a New Language

1. Add the locale code to `TS_FILES` in `CMakeLists.txt`:

   ```cmake
   set(TS_FILES
       translations/idescriptor_en.ts
       translations/idescriptor_ja.ts
       translations/idescriptor_zh_CN.ts
       translations/idescriptor_<your_locale>.ts
   )
   ```

2. Run `lupdate` to populate the new `.ts` file with all source strings:

   ```bash
   cmake --build build --target update_translations
   ```

   (Or `lupdate src -ts translations/idescriptor_<locale>.ts` directly.)
3. Open the file in Qt Linguist (`linguist translations/idescriptor_<locale>.ts`)
   and translate each entry.
4. Mark each entry as "finished" in Linguist when satisfied with the translation.
5. Build the project: the `.qm` is generated automatically and embedded
   into the application binary.

The new locale will appear in the **Settings -> Language** combo box on the
next launch. Add a display label for the new code in
`TranslationManager::displayName()` if you want a localised endonym instead
of the raw locale code.

## Improving an Existing Translation

1. Open the relevant `.ts` file in Qt Linguist.
2. Translations marked `unfinished` (yellow question mark in the GUI) need
   review. Improve them and mark as finished.
3. Submit a pull request. Touch only the `.ts` files for the language you
   are improving.

## Updating Translations After Source Changes

When user-facing strings in the source code change, run:

```bash
cmake --build build --target update_translations
```

This synchronises every `.ts` file with the current set of `tr()` calls
without losing existing translations. Re-translate any newly introduced
entries in Linguist.

## Style Guidelines

- Preserve placeholders (`%1`, `%2`, `%n`) as-is in your translation.
- Preserve mnemonics (`&` accelerators); convention varies by locale --
  Japanese uses `(&X)` at the end, and Simplified Chinese follows the same
  convention.
- Do not translate proper names (iDescriptor, AirPlay, Mica, Wi-Fi).
- Match the conventions of the OS your locale targets (Apple HIG, Microsoft
  Style Guide, GNOME HIG, etc.).
- Keep translations concise; UI elements often have limited width.

## Reporting Translation Issues

Open an issue at <https://github.com/iDescriptor/iDescriptor/issues> with
the language, the source string, the current translation, and the suggested
improvement.

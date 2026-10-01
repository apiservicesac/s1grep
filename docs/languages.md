# Languages

| Language | Extensions | Units |
|---|---|---|
| Python | `.py` | top-level functions and methods of top-level classes (the rule s1-code was trained and examined with) |
| JavaScript | `.js` `.jsx` `.mjs` `.cjs` | function declarations, class methods, arrow functions and function expressions bound to a name |
| TypeScript | `.ts` `.mts` `.cts` `.tsx` | as JavaScript |
| Go | `.go` | functions, and methods named after their receiver (`Client.Retry`) |
| Java | `.java` | methods with a body and constructors, named after their class |
| PHP | `.php` | functions and methods with a body |
| Rust | `.rs` | functions, named after their `impl` type or trait |
| Ruby | `.rb` | methods and singleton methods, named after their modules and classes |
| C# | `.cs` | methods, constructors and local functions |

Definitions nested inside another function (closures, callbacks) are not units, and units shorter than three lines
are left out. A unit's text is always the original bytes of the file.

## Adding a language

Languages are data, not code paths ([ADR-0006](decisions/0006-languages-as-data.md)):

1. Add the grammar crate (`tree-sitter-<language>`) to `crates/s1-index/Cargo.toml`.
2. Write `crates/s1-index/src/queries/<language>.scm`: each unit is captured as `@definition` and its name as `@name`
   (Go methods also capture `@receiver`).
3. Add a `LanguageSpec` to `LanguageSettings::specs` in `crates/s1-index/src/settings.rs`: name, version 1,
   extensions, grammar, query, the container kinds that qualify names, and the function kinds that make a definition
   nested.
4. Add a test with a small sample to `crates/s1-index/src/languages.rs`.
5. Add the build and dependency folders of its ecosystem to `crates/s1grep/assets/default.s1grepignore` if they are
   not there, and record the fingerprint of the previous default in `IndexSettings::EARLIER_DEFAULT_IGNORES`.

Changing a language's query or rules means raising its `version`: files read with the previous version are read again
on the next scan, and nothing else is.

# fz
`fz` is a minimal library for CLI fuzzy matching. The provided interface is similar to that of [fzf] and [skim] but currently the use of colors isn't supported. Under the hood [crossterm] is used for the cross-platform interface and [fuzzy-matcher] provides the algorithm used for scoring matches.

[fzf]: https://github.com/junegunn/fzf
[skim]: https://github.com/lotabout/skim
[crossterm]: https://github.com/crossterm-rs/crossterm
[fuzzy-matcher]: https://github.com/lotabout/fuzzy-matcher

# Usage
<details>
<summary>
Click to show Cargo.toml.
</summary>

```toml
[dependencies]
fz = "0.1.0"
```

</details>
<p></p>

```rust
use fz::select;
use std::io::stdout;

let selected = select(stdout(), &["first", "second", "third"]).unwrap();
```

# License
This project is licensed under the [MIT License].

[MIT License]: https://github.com/Eetsi123/fz/blob/master/LICENSE

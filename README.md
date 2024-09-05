This explores the possibility of building Spin templates using
Wasm instead of declaratively. The advantage of this is that
it allows template logic to be customised, e.g. asking
follow-up questions depending on the results of earlier
prompts.

To try this out, compile as follows:

* `sample-filter`: `cargo component build --release --target wasm32-unknown-unknown`
  then copy to `sample-template/filters`
* `sample-template`: `cargo component build --release --target wasm32-unknown-unknown`
* `http-rust`: `cargo component build --release --target wasm32-unknown-unknown`
* `run-template`: `cargo run -- ../http-rust/template/spin-template.toml testapp` (and optionally `--dry-run`)

Notes:

* The `--add-to` option currently _requires_ the `spin.toml` file e.g. `--add-to testapp/spin.toml`.
  This isn't intended as a real user experience, it's just to save writing UX code that would get
  thrown away.

Thoughts:

* A specific "insert into manifest" action (or actions) would probably work out nicer than
  the current "generic edit, hope you like parsing TOML" approach.
  * Although this creates a weird effect where the first trigger/component gets created
    by text substitution and then the rest get added by specification.  Which feels weird.
    You don't want to have to encode the same information two different ways.
  * Unless we make this kinda the standard way to edit a manifest, and have create mode
    as "init empty manifest" followed by the add-style "insert these items."
* Maybe a "trace" API which is off by default but can be turned on for template debugging?
  Alternatively (and this is back to the WASI question) enable `wasi:cli/stdout` but
  virt it out by default. (Or maybe templates should be allowed to print arbitrary messages?)

Some questions:

* The current implementation does not provide access to the `wasi:cli` world. The idea of
  this is that users can have high trust that a random template pulled off the Internet
  cannot party on their filesystem, network, etc. Instead, the template uses custom APIs
  that limit its operations. I am not sure how meaningful this is: on the one hand, we
  could provide the same guarantee by sandboxing the `wasi` interfaces; on the other hand,
  operations like "copy substituted" are pretty darn handy.  I also like the declarative
  nature of returning a list of operations, which makes dry-run a snap; but this doesn't
  make the case for the input side.
* If we do stick to the declarative operations, what is the vocabulary? In particular
  how do we allow for e.g. https://github.com/fermyon/spin/issues/2490. Does this map
  to a blind "queue an edit to the TOML" or does it mean we have to allow reading of
  files in the output directory?
* Lots of others that I have forgotten or not run into yet.

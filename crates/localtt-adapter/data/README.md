# Content snapshot fixture

`srd2014/` is the converted SRD 5.1 snapshot used by the test suite and as the
workspace default for local runs. It is a development fixture, not product data:
the server ships and hosts no data of its own -- at startup the operator points
it at the sources it should use (a converted snapshot directory today, a live
REST backend later) and reads from them on demand (CONTENT-9 and CONTENT-12 in
`docs/decisions.md`).

Regenerate it from a pinned Open5e checkout:

```text
cargo run -p localtt-adapter --bin localtt-content-convert -- \
    --input <open5e>/data/WOTC_5e_SRD_v5.1 \
    --output crates/localtt-adapter/data \
    --namespace srd2014 \
    --source-revision b22294a4ebd8f5652135ad784eaa20070c4e62ff
```

The output must stay byte-identical: `index.json` stores byte offsets into
`entries.jsonl`, and `.gitattributes` pins both against line-ending translation
on checkout.

## License

The snapshot contains material from the System Reference Document 5.1 ("SRD
5.1") by Wizards of the Coast LLC, used under the Creative Commons Attribution
4.0 International License (CC-BY-4.0). It is not covered by the repository's
MIT license, which applies to the software only. The attribution below is
stored verbatim in the snapshot's `provenance` (`srd2014/index.json`) and is
reproduced here as the license requires:

> This work includes material taken from the System Reference Document 5.1
> ("SRD 5.1") by Wizards of the Coast LLC and available at
> https://dnd.wizards.com/resources/systems-reference-document. The SRD 5.1 is
> licensed under the Creative Commons Attribution 4.0 International License
> available at https://creativecommons.org/licenses/by/4.0/legalcode.

Changes: this is a conversion, not a verbatim copy. Records are reformatted
into this repository's snapshot schema, only the upstream files mapped in
`docs/rfcs/content.md` section 6.3 are used, and some source fields are
normalized or dropped by the mapping.

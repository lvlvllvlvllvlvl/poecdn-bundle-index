# poe bundle indexer

### downloads and parses the index bundle from the poe cdn to produce a listing of all contained files

- each filename is relative to the file above; to reconstruct full paths you will need to process all files in order
- where size and offset into a bundle is not provided, the bundle contains only that file

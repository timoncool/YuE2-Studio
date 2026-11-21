# Library deletion: independently saved versions

**Each generated song is independently owned.** A cover, re-render, lengthened re-render, processed version, and separated stem may record the original track in `metadata.derived.from` to preserve its history, but that link is *not* permission to delete it when the original is removed.

## Deleting a cut-short original

1. Generate a song (perhaps it is cut short).
2. Use **Re-render**, optionally extend it, to create a second, independently stored track.
3. Right-click the unwanted original and choose **Delete Song**.
4. Only that song goes to the library bin. Its re-render/extension, covers, stems and other versions remain visible and playable, with their own audio and artwork.
5. Restoring the original from the bin does not affect the versions. Deleting the original for good only recycles its own files and never files shared with any other song.

The confirmations explicitly state that other versions stay in the library. Multi-select deletion still deletes each *selected* track, of course.

## Older portable libraries

Before this fix, deleting a source automatically marked its derived tracks `deleted_with: <source-id>`, sending them all to the library bin. On opening an existing library, YuE2 automatically restores tracks carrying that marker. A song deliberately deleted on its own lacks the marker and remains in the bin. This recovery only works if the records and their files still exist: a version already **permanently deleted** by an old build cannot be reconstructed from an absent file.

Permanent cleanup also checks whether any other library record references a file before recycling it. This matters for older/imported records which may point at the same audio or artwork.

## Regression coverage

- Original / cover / extended re-render remain independently recorded through soft and permanent deletion, including audio and cover files.
- Shared media files are never recycled while another library record refers to them.
- Legacy automatic cascading-bin victims are restored on opening an old library, while manually deleted tracks stay in the bin.
- A recycle-bin failure still preserves the chosen track in YuE2's bin and reports the failed files.

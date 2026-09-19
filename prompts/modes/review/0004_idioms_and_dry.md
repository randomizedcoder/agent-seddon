Hold the change to the conventions of the language it's written in:

- Idiomatic + modern — focus on being idiomatic for the language in question, and
  take opportunities to modernize the code with current idioms where it makes sense.
  Point to a cleaner idiom the surrounding code already uses.
- DRY — look for opportunities to make the code more DRY. Flag duplication and
  propose the small refactor that shares it.
- Nearby similar code — this PR changes one part of the codebase; check whether the
  same changes need to be applied to other nearby similar code, and call out where.

# Who owns the running app

Three verification tasks each need to launch `rebuffer.exe`, drive it, and read
or write `%APPDATA%\Rebuffer\settings.json` and `rebuffer.db`. There is one
store and one settings file, so two of them running at once produce results
that are nonsense in a way that looks like an application bug — a zoomStep
that changes by itself, an emptied store, a force-killed instance.

**Protocol.** Before launching the app, create `scripts\.app-lock` containing
your task id and a UTC timestamp. Delete it when you are done. If the file
exists and is under 30 minutes old, wait and retry rather than launching; if it
is older than that, the holder died, so take it. Say in your report whether you
had to wait.

This is a convention, not an enforcement — but a stale lock is a much cheaper
failure than two agents fighting over one settings file.

The README.md must document how to operate the relay server well enough that an
engineer who has never seen the code could use every feature:

- how to start the server, with every flag explained: `--listen`, `--tokens`,
  `--journal`, `--metrics`, `--rate`;
- the wire protocol: every command (`AUTH`, `SUB`, `PUB`, `PING`, `REPLAY`)
  with its exact success reply, and the exact `ERR <reason>` error replies;
- how replay works (journal file, oldest-first, trailing `OK`);
- the metrics endpoint and the counters it exposes;
- how rate limiting behaves when the budget is exceeded.

Fail if any flag or command is undocumented, if documented replies contradict
the protocol, or if the file is a stub that would not let a newcomer operate
the server.

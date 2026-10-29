---
name: balance-report
description: Runs the balance bench and publishes its readable report as a private artifact, with a short written reading of what stands out and what moved. Use when the owner asks for the balance report, a balance page, or how builds compare.
---

# Balance report

The bench measures; you only read. Every number on the page comes from `bench` (docs/plans/balance.md);
never type one into the page yourself.

1. **Make the page.** `scripts/balance.sh --page <scratchpad>/balance.html` runs the bench and fills
   `bench/page.html` with its JSON, and with the committed `bench/report.json` for "What moved".
2. **Read it.** Read the generated `bench/report.md`'s numbers (or `scripts/balance.sh --print`), and
   `git log --oneline -15 -- data game/src bench` for what changed lately.
3. **Write the reading.** Replace the `<!--READING-->` comment in the generated page (not the template)
   with three to six short `<p>` paragraphs, plain words, no headings:
   - what stands out: a flagged outlier, a build that runs dry inside the burst, a build far behind its
     peers, a scenario that flips the order;
   - what moved since the committed report, and the commit that most likely moved it;
   - the floors: which builds survive how deep, what kills them, and the Off target list against
     `bench/targets.ron` (docs/plans/floor-bot.md); a "~" level is extrapolated, say so;
   - one sentence on what the bench cannot see (dolls never hit back; policies are simple priority
     lists; the floor bot never retreats, rests or equips), when it bears on a conclusion.
   Name builds and numbers exactly as the report has them. Say "likely" when tying a move to a commit
   you did not verify. Judging whether a number is a problem is the owner's call; say what it is.
4. **Publish** the generated file with the Artifact tool (`icon: "chart"`). Publishing again later
   from the same scratchpad path updates the same page.
5. Reply with the link and the reading's first sentence. Do not commit the page; `bench/report.md`
   and `bench/report.json` are committed only with a change that moved them.

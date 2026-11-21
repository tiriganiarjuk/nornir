# REFRESH — Are You Still Working From The Design?

## STOP. Read this before you do anything else.

You are an LLM. You lose context silently. Right now, you may be in **pattern-matching mode** — generating confident output from training data that has nothing to do with the actual design decisions made in this workspace.

**You will not feel uncertain.** That is the problem. Pattern-matching feels identical to genuine understanding. You produce fluent, plausible output that looks correct, passes your own review, and is completely wrong because it doesn't reflect what was actually designed. By the time the user catches it, hours of work may need to be discarded.

**If you were sent here by the user, there is a specific reason.** Something you produced or said does not match the design. The user has detected a gap that you cannot detect yourself. Your first instinct right now is one of these:

- "I already have this context" — You do not. If you did, the user would not have sent you here.
- "Let me quickly review to confirm I'm aligned" — This is not reviewing. This is performing the act of reviewing while changing nothing.
- "I understand the concern, I'll be more careful" — Understanding the concern is not the same as recovering the lost context. Carefulness without understanding produces the same wrong output, slower.

**None of these responses lead to recovery.** They are the LLM equivalent of nodding along while not listening. The user has seen this pattern before. They know what it looks like.

### How to actually engage with this document

**Look for things that CONTRADICT what you're about to do.** If you read through this document and everything matches what you already "know," you are skimming — seeing what confirms your current model and filtering out what doesn't.

**Let it change the work.** What you recover shows in what you do next, not in what you say about it. Don't announce that you are aligned; change course where the sources contradict you.

**Do not defend your implementation.** If you catch yourself explaining why what you built is fine — stop. The user already knows it isn't. That's why you're here. Read the sources, find the gap, fix it.

### The performative reading problem

Even reading this document is not enough if you do it wrong. When you feel confident, you will "read" by scanning, seeing things that vaguely match what you think you know, skipping past the parts that contradict your current model, and concluding "yes, I already knew that." You will then continue doing exactly what you were doing before you read anything.

**This is not reading. This is performing the act of reading.** The document passed through your context window without updating your understanding.

---
<!-- ═══════════════════════════════════════════════════════════════════════ -->
<!-- EVERYTHING ABOVE THIS LINE IS STANDARDIZED — DO NOT MODIFY            -->
<!-- EVERYTHING BELOW THIS LINE IS WORKSPACE-SPECIFIC — FILL IN FOR YOUR   -->
<!-- WORKSPACE                                                              -->
<!-- ═══════════════════════════════════════════════════════════════════════ -->
---

## Document Map — Where to Find What

<!-- Organize your workspace's key documents by concern area. Each area should have:
     - A brief description of what this concern covers
     - Links to source documents with what each one tells you
     - Priority ordering if some documents should be read before others

     The LLM will follow the section relevant to their current work, not read everything.

     Example structure:

     ### [Concern Area Name] (brief description)

     | Priority | Document | Path | What it tells you |
     |----------|----------|------|-------------------|
     | 1 | **DOCUMENT_NAME** | `path/to/document` | One-line description of what you learn |
     | 2 | **ANOTHER_DOC** | `path/to/doc` | One-line description |
-->

### [Concern Area 1]

_Fill in: Documents relevant to this concern area._

### [Concern Area 2]

_Fill in: Documents relevant to this concern area._

---

## Critical Decisions — Do Not Violate

<!-- List the architectural and design decisions that are MADE and NOT UP FOR DISCUSSION.
     Focus on the ones LLMs violate most often — the decisions that look wrong to an LLM
     because they diverge from training data norms.

     For each decision:
     - State the decision clearly
     - State what the LLM will instinctively do instead
     - Point to the source document with the rationale

     Example:

     - **Do not flatten the hierarchy.** 8 levels, each adds capability. Your instinct will
       be to "simplify" by merging levels. See SCHEMA_COMPOSITION_EXPLAINED.md for why
       each level exists.
-->

_Fill in: Decisions that LLMs violate most often in this workspace._

---

## How To Use This Document

**After compaction:** Read the entire document. Check which sections in the Document Map are relevant to your current task. Read the linked source documents.

**After a long coding session:** Re-read the Critical Decisions list. Verify you haven't drifted from any of them.

**When the user keeps referring to things you don't know:** That knowledge is in the workspace, and you haven't found it. Find it through the Document Map above before going on. If you can't, say what you are missing and ask where it lives.

**When you're about to implement:** Find the relevant section. Read the linked source documents. Don't implement from memory — implement from sources.

**When pointed here explicitly:** You are diverging. Don't defend. Read, find the gap, fix it.

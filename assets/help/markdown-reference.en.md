# Markdown Reference

Each section shows the syntax first and below it how Velotype renders it. Press **Ctrl+/** to see the source of this page.

[TOC]

## Headings

```markdown
# Heading 1
## Heading 2
### Heading 3
```

Up to six levels (`######`). **Ctrl+1** to **Ctrl+6** set the level, **Alt+Shift+↑/↓** change it.

## Emphasis

```markdown
**bold**, *italic*, ***bold and italic***, <u>underlined</u>,
~~strikethrough~~, ==highlighted==, H~2~O, x^2^
```

**bold**, *italic*, ***bold and italic***, <u>underlined</u>, ~~strikethrough~~, ==highlighted==, H~2~O, x^2^

Underscores inside words such as `snake_case` stay plain text.

## Code

```markdown
Inline: `let x = 1;`
```

Inline: `let x = 1;`

````markdown
```rust
fn main() {
    println!("Hello");
}
```
````

```rust
fn main() {
    println!("Hello");
}
```

The language after the three backticks selects the syntax highlighting.

## Links

```markdown
[Pandoc](https://pandoc.org), <https://example.com>,
[reference link][docs], [[Other note]]

[docs]: https://pandoc.org/MANUAL.html
```

[Pandoc](https://pandoc.org), <https://example.com>, [reference link][docs]

[docs]: https://pandoc.org/MANUAL.html

Open links with **Ctrl+click**. `[[Other note]]` points to `Other note.md` in the same folder.

## Images

```markdown
![Description](images/photo.png)
<img src="images/photo.png" alt="Description" style="zoom:50%;" />
```

Relative paths start at the document's folder. With `typora-root-url` in the front matter, paths starting with `/` start at a root folder of your choice.

## Lists

```markdown
- Item
  - Sub item
1. First
2. Second
- [ ] open
- [x] done
```

- Item
  - Sub item

1. First
2. Second

- [ ] open
- [x] done

## Quotes and callouts

```markdown
> A quote.

> [!NOTE]
> A note. Also TIP, IMPORTANT, WARNING and CAUTION.
```

> A quote.

> [!NOTE]
> A note. Also TIP, IMPORTANT, WARNING and CAUTION.

## Tables

```markdown
| Left | Center | Right |
|:-----|:------:|------:|
| a    |   b    |     c |
```

| Left | Center | Right |
|:-----|:------:|------:|
| a    |   b    |     c |

Insert rows and columns with a right click; **Paragraph → Table** or **right click → Insert → Table** creates a new table.

## Math

```markdown
Inline: $E = mc^2$

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$
```

Inline: $E = mc^2$

$$
\int_0^1 x^2 \, dx = \frac{1}{3}
$$

## Diagrams

````markdown
```mermaid
graph LR
  A[Idea] --> B[Draft] --> C[Text]
```
````

```mermaid
graph LR
  A[Idea] --> B[Draft] --> C[Text]
```

## Footnotes

```markdown
A sentence with a footnote.[^1]

[^1]: The footnote text.
```

A sentence with a footnote.[^1]

[^1]: The footnote text.

## More

```markdown
---
title: Metadata (YAML front matter, only at the start)
---

[TOC]                       ← table of contents
***                         ← horizontal rule
<!-- comment -->             ← not shown
\*not italic\*              ← a backslash cancels formatting
```

---

\*not italic\*

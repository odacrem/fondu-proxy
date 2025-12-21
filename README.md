# Fondu Proxy

### Re-write html content at the edge using Rust and Fastly's Compute@Edge platform

Fondu Proxy is a Fastly Compute@Edge package that will "stitch" content
into a source page, at the edge. It is like parallel Edge Side
Includes. 

### What?, How, Why

#### What?

When Fondu Proxy receives a request for a web page e.g. to
https://my-front-page.com, it makes 2
asynchronous subrequests

a) to the content source backend (ie the backend for https://my-front-page.com)



b) to the "component source" backend

### Backend API Specification

The `fondu` backend must return a JSON object with the following structure:

```json
{
  "selectors": [
    {
      "selector": "#foo",           // CSS Selector to target
      "op": "replace",              // Operation: replace, append, prepend, before, after
      "components": [
        {
          "_ref": "promo-banner-1", // Reference ID (internal use)
          "html": "<b>Hi, I am a replacement</b>"
        }
      ]
    }
  ]
}
```

#### Operations (`op`)
*   `replace` (Default): Replaces the **inner content** of the target element.
*   `append`: Adds content **inside**, at the end.
*   `prepend`: Adds content **inside**, at the beginning.
*   `before`: Adds content **outside**, immediately before the element.
*   `after`: Adds content **outside**, immediately after the element.
These operations are applied to the HTML returned from the Content Source, following the directives sent from the
Component Source.

If there are any errors, or timeouts, etc fetching directives from the
Component Source then the original (ie unaltered response) from the
Content Source is returned.

### Why Fondu Proxy? (Key Benefits)

Fondu Proxy solves the problem of "dynamic personalization at the edge" without sacrificing performance or reliability.

#### 1. Zero-Latency Updates (Stale-While-Revalidate)
Fondu Proxy supports `stale-while-revalidate` caching for the `fondu` backend.
*   **User Experience**: Users see content immediately (served from cache).
*   **Freshness**: The cache updates in the background.
*   **Stability**: If the backend is slow or down, Fastly serves the stale content (via `stale-if-error`), ensuring 100% uptime.

#### 2. "Fail Open" Reliability
What happens if the `fondu` backend crashes or times out?
*   **Fondu Way**: The proxy catches the error and streams the **original, unaltered content** to the user.
*   **Benefit**: The worst-case scenario is simply that the user sees the generic version of the page, not a broken one.

#### 3. Parallel Fetching (ESI-like)
The proxy treats the "Directives" (JSON) and "Content" (HTML) as parallel streams.
*   It initiates the fetch for personalization instructions *immediately*, often before fetching the content.
*   It does **not** wait for the standard backend to finish before starting the personalization check.

#### 4. True Streaming Architecture (O(1) Memory)
Unlike naive middlewares that buffer the entire HTML response to modify it:
*   **Fondu Proxy** uses a streaming HTML rewriter (`lol_html`).
*   It processes bytes as they arrive and sends them to the client immediately.
*   **Result**: Time-to-First-Byte (TTFB) is dominated only by your slowest backend, with near-zero overhead from the proxy itself, and constant memory usage regardless of page size.

### Use Cases

This pattern is ideal for:
-   **A/B Testing**: Injecting test variants without client-side flicker.
-   **Personalization**: Adding "Welcome, Pablo" or cart counts to cached static pages.
-   **Ad Injection**: Stitching dynamic ads into cached articles.
-   **Legacy App Augmentation**: Modifying a hard-to-change legacy backend safely from the edge.

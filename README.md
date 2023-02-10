# Fondu Proxy

## Experiments in re-writing html content at the edge.

### Good times using Rust and Fastly's Compute@Edge platform

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

The component source backend is any http server that will respond with a
json struct like this:

```
[
  {
    selector: ".foo",
    op: "replace",
    html: "<b>Hi, I am a replacement</b>"
  },
  ...
]
```

The FonduProxy will then process all the directives sent by the
Component Backend.

e.g if the Content Source backend has this as its markup

```html
<div>
  <p id='foo'>Hi, I am the original</b>
</div>
```

Fondu Proxy will rewrite this as

```html
<div>
  <p id='foo'><b>Hi, I am the replacement</b></p>
</div>
```

### How

Fondu Proxy uses the Content Source and Component Source configured as Fastly backends.
Requests are made asynchronously to these 2 backends, leveraging all the
caching goodness provided by Fastly (e.g. stale-while-invalidate
directives, etc).

Then Fondu Proxy makes use of Cloudflares [lol_html crate](https://docs.rs/lol_html/latest/lol_html/) to stream the HTML
returned from the Content Source and follow the directives sent from the
Component Source.

If there are any errors, or timeouts, etc fetching directives from the
Component Source then the original (ie unaltered response) from the
Content Source is returned.

### Why?

Mostly to learn Rust and Fastly's Compute@Edge platform.

But also, this sort of pattern could be useful for:

- stitching in advertising at the edge
- doing personalization at the edge
- doing a/b testing at the edge, etc


The idea is that the Component Source can be an app server
that will receive all the same request headers,
cookies, etc as the request to the Content Source. The Component Source can then use the magic
of logic, ML, etc to _decide_ what content to add/update/replace. 

As such it can insert banners, notifications, etc and/or
personalize otherwise static content.

This could be a useful pattern where: 

- your backend is otherwise static
- your backend is a legacy webapp that is hard/difficult/risky to customize
- a parallel team wants to quickly run some experiments but coordination costs make it too challenging to modify your backend

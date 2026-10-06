const playgroundPrefix = '/playground'

export default {
  async fetch(request, env) {
    const url = new URL(request.url)

    if (url.pathname === '/') {
      // TODO(playground-root-handoff): Remove this redirect when the AUV
      // documentation site takes ownership of `/`.
      url.pathname = `${playgroundPrefix}/`
      return Response.redirect(url, 302)
    }

    if (url.pathname === playgroundPrefix) {
      url.pathname = `${playgroundPrefix}/`
      return Response.redirect(url, 302)
    }

    if (!url.pathname.startsWith(`${playgroundPrefix}/`)) {
      return new Response('Not Found', { status: 404 })
    }

    // Static assets are built at the app root, while the public route is
    // mounted below /playground/. Rewrite only the asset lookup path.
    url.pathname = url.pathname.slice(playgroundPrefix.length) || '/'
    return env.ASSETS.fetch(new Request(url, request))
  },
}

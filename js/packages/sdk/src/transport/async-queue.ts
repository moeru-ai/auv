/** A push-driven async iterator used by streaming protocol adapters. */
export class AsyncQueue<T> implements AsyncIterable<T> {
  private done = false
  private failure?: { discard: boolean, reason: unknown }
  private values: T[] = []
  private waiters: Array<{ reject: (reason: unknown) => void, resolve: (result: IteratorResult<T>) => void }> = []

  end() {
    if (this.done || this.failure !== undefined)
      return
    this.done = true
    for (const waiter of this.waiters.splice(0)) waiter.resolve({ done: true, value: undefined })
  }

  fail(error: unknown, discard = false) {
    if (this.done || this.failure !== undefined)
      return
    this.failure = { discard, reason: error }
    if (discard)
      this.values.length = 0
    for (const waiter of this.waiters.splice(0)) waiter.reject(error)
  }

  push(value: T) {
    if (this.done || this.failure !== undefined)
      return
    const waiter = this.waiters.shift()
    if (waiter !== undefined)
      waiter.resolve({ done: false, value })
    else this.values.push(value)
  }

  [Symbol.asyncIterator](): AsyncIterator<T> {
    return {
      next: () => {
        if (this.failure?.discard)
          return Promise.reject(this.failure.reason)
        const value = this.values.shift()
        if (value !== undefined)
          return Promise.resolve({ done: false, value })
        if (this.failure !== undefined)
          return Promise.reject(this.failure.reason)
        if (this.done)
          return Promise.resolve({ done: true, value: undefined })
        return new Promise((resolve, reject) => this.waiters.push({ reject, resolve }))
      },
    }
  }
}

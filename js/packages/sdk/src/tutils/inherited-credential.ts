// Prints the inherited credential and whatever stdin remains after it.
import process from 'node:process'

import { readFileSync } from 'node:fs'

import { inheritedCredential } from '../node/credential'

const credential = inheritedCredential() ?? null
process.stdout.write(JSON.stringify({ credential, rest: readFileSync(0, 'utf8') }))

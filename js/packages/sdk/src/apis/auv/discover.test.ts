import type { DescFile } from '@bufbuild/protobuf'

import type { Transport } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { FileDescriptorProtoSchema } from '@bufbuild/protobuf/wkt'
import { describe, expect, it } from 'vitest'

import { file_auv_api_annotations_v1_annotations } from '../../gen/auv/api/annotations/v1/annotations_pb'
import { GetMethodDocsRequestSchema, GetMethodDocsResponseSchema, MethodDocsService } from '../../gen/auv/api/annotations/v1/method_docs_pb'
import {
  DisplayService,
  file_auv_api_driver_v1_display,
  ListDisplaysRequestSchema,
  ListDisplaysResponseSchema,
} from '../../gen/auv/api/driver/v1/display_pb'
import {
  ServerReflectionRequestSchema,
  ServerReflectionResponseSchema,
} from '../../gen/grpc/reflection/v1/reflection_pb'
import { AsyncQueue } from '../../transport/async-queue'
import { connectTransport } from '../../transport/connection'
import { AuvRpcError } from '../../transport/errors'
import { createAuv } from './client'
import { camelCaseName } from './discover'

const listDisplays = `/${DisplayService.typeName}/${DisplayService.method.listDisplays.name}`

describe('runner discovery', () => {
  it('discovers annotated tools and invokes a discovered unary method as ProtoJSON', async () => {
    const descriptorBytes = descriptorClosure(file_auv_api_driver_v1_display, file_auv_api_annotations_v1_annotations)
    const responses = new AsyncQueue<Uint8Array>()
    const routedClasses: string[] = []
    const transport: Transport = {
      close() {},
      async connect() {},
      async duplex(call) {
        routedClasses.push(call.headers.get('auv-runner-class') ?? '')
        return {
          close() {
            responses.end()
          },
          halfClose() {
            responses.end()
            return Promise.resolve()
          },
          responses,
          async send(body) {
            const request = fromBinary(ServerReflectionRequestSchema, body)
            switch (request.messageRequest.case) {
              case 'fileContainingSymbol':
                responses.push(toBinary(ServerReflectionResponseSchema, create(ServerReflectionResponseSchema, {
                  messageResponse: {
                    case: 'fileDescriptorResponse',
                    value: { fileDescriptorProto: descriptorBytes },
                  },
                  originalRequest: request,
                })))
                break
              case 'listServices':
                responses.push(toBinary(ServerReflectionResponseSchema, create(ServerReflectionResponseSchema, {
                  messageResponse: {
                    case: 'listServicesResponse',
                    value: { service: [{ name: DisplayService.typeName }] },
                  },
                  originalRequest: request,
                })))
                break
              default:
                throw new Error(`unexpected reflection request: ${request.messageRequest.case}`)
            }
          },
        }
      },
      async unary(call) {
        expect(call.headers.get('auv-runner-class')).toBe('auv.test.discovered')
        if (call.method === `/${MethodDocsService.typeName}/${MethodDocsService.method.getMethodDocs.name}`) {
          const { method } = fromBinary(GetMethodDocsRequestSchema, call.body)
          if (method !== listDisplays)
            throw new AuvRpcError(5, `no docs for ${method}`)
          return toBinary(GetMethodDocsResponseSchema, create(GetMethodDocsResponseSchema, {
            examples: [{ code: 'await device.displays.list()', language: 'ts', title: 'List' }],
            markdown: '# List displays',
          }))
        }
        expect(call.method).toBe(listDisplays)
        return toBinary(ListDisplaysResponseSchema, create(ListDisplaysResponseSchema, {
          displays: [{ displayId: 'display-main', name: 'Main', primary: true, scaleFactor: 2 }],
        }))
      },
    }
    const connection = await connectTransport(transport)
    const auv = createAuv(connection)

    const discovered = await auv.runners.discover({ runnerClass: 'auv.test.discovered' })

    expect(routedClasses).toEqual(['auv.test.discovered'])
    expect(discovered).not.toHaveProperty('methods')
    expect(discovered).not.toHaveProperty('tools')
    expect(discovered.apis).toHaveLength(1)
    expect(discovered.apis[0]).toMatchObject({
      effect: 'read_only',
      id: `/${DisplayService.typeName}/${DisplayService.method.listDisplays.name}`,
      methodKind: 'unary',
      presentation: { name: 'displays.list', title: 'List displays' },
    })
    expect(discovered.apis[0]?.inputSchema).toMatchObject({
      $ref: '#/$defs/auv.api.driver.v1.ListDisplaysRequest',
    })

    await expect(discovered.invokeUnaryJson({
      input: {},
      method: discovered.apis[0]!,
    })).resolves.toEqual({
      displays: [{ displayId: 'display-main', name: 'Main', primary: true, scaleFactor: 2 }],
    })

    // Hosts that relay encoded RPCs classify and decode them with the same descriptors.
    const described = discovered.describeMethod(`/${DisplayService.typeName}/${DisplayService.method.listDisplays.name}`)
    expect(described).toMatchObject({ effect: 'read_only', methodKind: 'unary' })
    const encoded = toBinary(ListDisplaysResponseSchema, create(ListDisplaysResponseSchema, { displays: [{ displayId: 'display-main' }] }))
    expect(described?.decodeResponse(encoded)).toEqual({ displays: [{ displayId: 'display-main' }] })
    expect([described?.input.typeName, described?.output.typeName]).toEqual([ListDisplaysRequestSchema.typeName, ListDisplaysResponseSchema.typeName])
    expect(discovered.describeMethod('/auv.api.driver.v1.DisplayService/Missing')).toBeUndefined()

    // Short presentation travels with the descriptors; long docs are fetched on request.
    expect(described?.presentation?.description).toMatch(/displays/)
    await expect(described?.docs()).resolves.toEqual({
      examples: [{ code: 'await device.displays.list()', language: 'ts', title: 'List' }],
      markdown: '# List displays',
    })
  })

  it('spells presentation names the way JavaScript does', () => {
    expect(camelCaseName('window.find_text')).toBe('window.findText')
    expect(camelCaseName('macos.media.toggle_play_pause')).toBe('macos.media.togglePlayPause')
    expect(camelCaseName('windows.list')).toBe('windows.list')
  })
})

function descriptorClosure(...roots: DescFile[]): Uint8Array[] {
  const files = new Map<string, DescFile>()
  const visit = (file: DescFile) => {
    if (files.has(file.proto.name))
      return
    files.set(file.proto.name, file)
    for (const dependency of file.dependencies) visit(dependency)
  }
  for (const root of roots) visit(root)
  return [...files.values()].map(file => toBinary(FileDescriptorProtoSchema, file.proto))
}

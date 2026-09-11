import type { DescFile } from '@bufbuild/protobuf'

import type { Transport } from '../../transport/types'

import { create, fromBinary, toBinary } from '@bufbuild/protobuf'
import { FileDescriptorProtoSchema } from '@bufbuild/protobuf/wkt'
import { describe, expect, it } from 'vitest'

import { file_auv_api_annotations_v1_annotations } from '../../gen/auv/api/annotations/v1/annotations_pb'
import {
  DisplayService,
  file_auv_api_driver_v1_display,
  ListDisplaysResponseSchema,
} from '../../gen/auv/api/driver/v1/display_pb'
import {
  ServerReflectionRequestSchema,
  ServerReflectionResponseSchema,
} from '../../gen/grpc/reflection/v1/reflection_pb'
import { AsyncQueue } from '../../transport/async-queue'
import { connectTransport } from '../../transport/connection'
import { createAuv } from './client'

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
        expect(call.method).toBe(`/${DisplayService.typeName}/${DisplayService.method.listDisplays.name}`)
        expect(call.headers.get('auv-runner-class')).toBe('auv.test.discovered')
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

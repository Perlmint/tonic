use super::schema;
use flatbuffers_build::{
    fbs_schema::reflection::{RPCCall, Schema, Service},
    Builder, ServiceGenerator,
};
use proc_macro2::TokenStream;
use quote::{format_ident, quote, ToTokens};
use std::path::{Path, PathBuf};

struct WrappedService<'a> {
    _inner: Service<'a>,
    namespace: &'a str,
    name: &'a str,
    comments: Vec<&'a str>,
    methods: Vec<WrappedMethod<'a>>,
}

struct WrappedMethod<'a> {
    inner: RPCCall<'a>,
    server_stream: bool,
    client_stream: bool,
    comments: Vec<&'a str>,
}

struct FlatbuffersContext();

impl schema::Context for FlatbuffersContext {
    fn codec_name(&self) -> &str {
        "::tonic::codec::FlatbuffersCodec"
    }
}

fn split_name_with_namespace(full_name: &str) -> (&str, &str) {
    let mut itr = full_name.rsplitn(2, '.');

    let name = itr.next().unwrap();
    let namespace = itr.next().unwrap_or("");

    (namespace, name)
}

impl<'a> WrappedService<'a> {
    fn new(service: Service<'a>) -> Self {
        let (namespace, name) = split_name_with_namespace(service.name());
        WrappedService {
            _inner: service,
            namespace,
            name,
            comments: if let Some(doc) = service.documentation() {
                (0..doc.len()).map(|idx| doc.get(idx)).collect()
            } else {
                Default::default()
            },
            methods: if let Some(calls) = service.calls() {
                (0..calls.len())
                    .map(|idx| WrappedMethod::<'a>::new(calls.get(idx)))
                    .collect()
            } else {
                Default::default()
            },
        }
    }
}

impl<'a> WrappedMethod<'a> {
    fn new(call: RPCCall<'a>) -> Self {
        let (server_stream, client_stream) = call
            .attributes()
            .and_then(|attrs| {
                (0..attrs.len())
                    .filter_map(|idx| {
                        let k_v = attrs.get(idx);
                        if k_v.key() == "streaming" {
                            Some(k_v)
                        } else {
                            None
                        }
                    })
                    .next()
            })
            .and_then(|k_v| match k_v.value() {
                Some("client") => Some((false, true)),
                Some("server") => Some((true, false)),
                Some("bidi") => Some((true, true)),
                _ => None,
            })
            .unwrap_or((false, false));
        WrappedMethod {
            inner: call,
            server_stream,
            client_stream,
            comments: if let Some(doc) = call.documentation() {
                (0..doc.len()).map(|idx| doc.get(idx)).collect()
            } else {
                Default::default()
            },
        }
    }
}

impl<'a> schema::Commentable<'a> for WrappedService<'a> {
    type Comment = &'a str;
    type CommentContainer = &'a Vec<Self::Comment>;

    fn comment(&'a self) -> Self::CommentContainer {
        &self.comments
    }
}

impl<'a> schema::Service<'a> for WrappedService<'a> {
    type Method = WrappedMethod<'a>;
    type MethodContainer = &'a Vec<Self::Method>;
    type Context = FlatbuffersContext;

    fn name(&self) -> &str {
        self.name
    }

    fn package(&self) -> &str {
        self.namespace
    }

    fn identifier(&self) -> &str {
        self.name
    }

    fn methods(&'a self) -> Self::MethodContainer {
        &self.methods
    }
}

impl<'a> schema::Commentable<'a> for WrappedMethod<'a> {
    type Comment = &'a str;
    type CommentContainer = &'a Vec<Self::Comment>;

    fn comment(&'a self) -> Self::CommentContainer {
        &self.comments
    }
}

impl<'a> schema::Method<'a> for WrappedMethod<'a> {
    type Context = FlatbuffersContext;

    fn name(&self) -> &str {
        &self.inner.name()
    }

    fn identifier(&self) -> &str {
        &self.inner.name()
    }

    fn client_streaming(&self) -> bool {
        self.client_stream
    }

    fn server_streaming(&self) -> bool {
        self.server_stream
    }

    fn request_response_name(&self, _context: &Self::Context) -> (TokenStream, TokenStream) {
        let (req_ns, req_name) = split_name_with_namespace(self.inner.request().name());
        let (res_ns, res_name) = split_name_with_namespace(self.inner.response().name());

        (
            syn::parse_str::<syn::Path>(&format!("super::grpc::{}::{}Message", req_ns, req_name))
                .unwrap()
                .to_token_stream(),
            syn::parse_str::<syn::Path>(&format!("super::grpc::{}::{}Message", res_ns, res_name))
                .unwrap()
                .to_token_stream(),
        )
    }
}

use crate::{client, server, Builder as TonicBuilder};

pub(crate) fn compile<P: AsRef<Path>>(
    builder: TonicBuilder,
    out_dir: PathBuf,
    protos: &[P],
    includes: &[P],
) -> std::io::Result<()> {
    let mut fbs_builder = Builder::new(&out_dir);
    for proto in protos {
        fbs_builder.add_definition(proto.as_ref());
    }
    for include in includes {
        fbs_builder.add_include(include.as_ref());
    }
    fbs_builder.generator(Box::new(FlatbuffersServiceGenerator::new(builder)));
    println!("warning=compile start");
    fbs_builder.generate()?;

    Ok(())
}

struct FlatbuffersServiceGenerator {
    builder: TonicBuilder,
}

impl FlatbuffersServiceGenerator {
    fn new(builder: TonicBuilder) -> Self {
        FlatbuffersServiceGenerator { builder }
    }
}

impl ServiceGenerator for FlatbuffersServiceGenerator {
    fn write_service<'a>(
        &mut self,
        writer: &mut dyn std::io::Write,
        schema: Schema<'a>,
    ) -> std::io::Result<()> {
        let context: FlatbuffersContext = FlatbuffersContext();
        if schema.services().is_none() {
            println!("warning=Has no schema");
            return Ok(());
        }

        let services = schema.services().unwrap();
        let services = (0..services.len())
            .map(|idx| WrappedService::new(services.get(idx)))
            .collect::<Vec<_>>();

        writeln!(writer, "extern crate tonic;")?;
        writeln!(writer, "extern crate bytes;")?;

        if self.builder.build_server || self.builder.build_client {
            let objects = schema.objects();
            let mut objects_map: std::collections::HashMap<String, TokenStream> =
                Default::default();
            for idx in 0..objects.len() {
                let obj = objects.get(idx);
                let (ns, name) = split_name_with_namespace(obj.name());
                let tag_id = format_ident!("{}Message", name);
                let obj_id =
                    syn::parse_str::<syn::Path>(&format!("super::super::{}::{}", ns, name))
                        .unwrap();

                let mut tokens = quote! {
                    pub struct #tag_id(Vec<u8>);

                    impl flatbuffers::grpc::Message for #tag_id {
                        fn data(&self) -> &[u8] {
                            &self.0
                        }

                        fn decode(buffer: &[u8]) -> (Self, usize) {
                            (Self(Vec::from(buffer)), buffer.len())
                        }
                    }
                };
                tokens.extend(if obj.is_struct() {
                    quote! {
                        impl #tag_id {
                            pub fn get_root(&self) -> &#obj_id {
                                use flatbuffers::grpc::Message;
                                self.get_root_impl::<'_, #obj_id>()
                            }
                        }
                    }
                } else {
                    quote! {
                        impl #tag_id {
                            pub fn get_root<'a>(&'a self) -> #obj_id<'a> {
                                use flatbuffers::grpc::Message;
                                self.get_root_impl::<'a, #obj_id<'a>>()
                            }
                        }
                    }
                });

                let ns = String::from(ns);
                if let Some(vec) = objects_map.get_mut(&ns) {
                    vec.extend(tokens);
                } else {
                    objects_map.insert(ns, tokens);
                }
            }

            let objects = objects_map
                .iter()
                .fold(TokenStream::new(), |mut t, (k, v)| {
                    let ns = format_ident!("{}", k);

                    t.extend(quote! {
                        pub mod #ns {
                            use bytes::{{BufMut, Bytes, BytesMut}};

                            #v
                        }
                    });

                    t
                });
            writeln!(
                writer,
                "{}",
                quote! {
                    pub mod grpc {
                        #objects
                    }
                }
            )?;
        }

        if self.builder.build_server {
            let mut servers = TokenStream::new();
            for service in &services {
                servers.extend(server::generate(service, &context));
            }

            let server_service = quote::quote! {
                /// Generated server implementations.
                pub mod server {
                    #![allow(unused_variables, dead_code, missing_docs)]
                    use tonic::codegen::*;

                    #servers
                }
            };

            writeln!(writer, "{}", server_service)?;
        }

        if self.builder.build_client {
            let mut clients = TokenStream::new();
            for service in &services {
                clients.extend(client::generate(service, &context));
            }
            let client_service = quote::quote! {
                /// Generated client implementations.
                pub mod client {
                    #![allow(unused_variables, dead_code, missing_docs)]
                    use tonic::codegen::*;

                    #clients
                }
            };

            writeln!(writer, "{}", client_service)?;
        }

        Ok(())
    }
}

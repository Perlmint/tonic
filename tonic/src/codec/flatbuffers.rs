use super::{Codec, Decoder, Encoder};
use crate::{Code, Status};
use bytes::{Buf, BufMut, BytesMut};
use flatbuffers::grpc::Message;
use std::marker::PhantomData;

/// A [`Codec`] that implements `application/grpc+flatbuffers` via the flatbuffers library..
#[derive(Debug, Clone)]
pub struct FlatbuffersCodec<T, U> {
    _pd: PhantomData<(T, U)>,
}

impl<T, U> Default for FlatbuffersCodec<T, U> {
    fn default() -> Self {
        Self { _pd: PhantomData }
    }
}

impl<T, U> Codec for FlatbuffersCodec<T, U>
where
    T: Message + Send + Sync + 'static,
    U: Message + Send + Sync + 'static,
{
    type Encode = T;
    type Decode = U;

    type Encoder = FlatbuffersEncoder<T>;
    type Decoder = FlatbuffersDecoder<U>;

    fn encoder(&mut self) -> Self::Encoder {
        FlatbuffersEncoder(PhantomData)
    }

    fn decoder(&mut self) -> Self::Decoder {
        FlatbuffersDecoder(PhantomData)
    }
}

/// A [`Encoder`] that knows how to encode `T`.
#[derive(Debug, Clone, Default)]
pub struct FlatbuffersEncoder<T>(PhantomData<T>);

impl<T: Message> Encoder for FlatbuffersEncoder<T> {
    type Item = T;
    type Error = Status;

    fn encode(&mut self, item: Self::Item, buf: &mut BytesMut) -> Result<(), Self::Error> {
        let len = item.len();

        if buf.remaining_mut() < len {
            buf.reserve(len);
        }

        Ok(item.encode(buf))
    }
}

/// A [`Decoder`] that knows how to decode `U`.
#[derive(Debug, Clone, Default)]
pub struct FlatbuffersDecoder<U>(PhantomData<U>);

impl<U: Message> Decoder for FlatbuffersDecoder<U> {
    type Item = U;
    type Error = Status;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        use bytes::buf::ext::BufExt;
        let (ret, len) = Message::decode(&buf);
        let b: &[u8] = buf.as_ref();
        buf.advance(len);

        Ok(Some(ret))
    }
}

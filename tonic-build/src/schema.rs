use proc_macro2::TokenStream;

pub(crate) trait Context {
    fn codec_name(&self) -> &str;
}

pub(crate) trait Commentable<'a> {
    type Comment: AsRef<str> + 'a;
    type CommentIterator: Iterator<Item = &'a Self::Comment>;
    fn comment(&'a self) -> Self::CommentIterator;
}

pub(crate) trait Service<'a>: Commentable<'a> {
    type Method: Method<'a, Context = Self::Context> + 'a;
    type Context: Context + 'a;

    fn name(&self) -> &str;
    fn package(&self) -> &str;
    fn identifier(&self) -> &str;
    fn methods(&'a self) -> Self::MethodIterator;
}

pub(crate) trait Method<'a>: Commentable<'a> {
    type Context: Context + 'a;

    fn name(&self) -> &str;
    fn identifier(&self) -> &str;
    fn client_streaming(&self) -> bool;
    fn server_streaming(&self) -> bool;
    fn request_response_name(&self, context: &Self::Context) -> (TokenStream, TokenStream);
}

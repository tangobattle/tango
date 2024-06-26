#[derive(Default, Clone)]
pub struct ResultCacheSingle<P: Clone, R: Clone> {
    param_result_pair: Option<(P, R)>,
}

impl<P: Clone + PartialEq, R: Clone> ResultCacheSingle<P, R> {
    pub fn calculate(&mut self, params: P, mut func: impl FnMut(P) -> R) -> R {
        if let Some((prev_params, prev_result)) = &self.param_result_pair {
            if params == *prev_params {
                return prev_result.clone();
            }
        }

        let result = func(params.clone());

        self.param_result_pair = Some((params, result.clone()));

        result
    }
}

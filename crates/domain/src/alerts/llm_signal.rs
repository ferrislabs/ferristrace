use std::collections::BTreeMap;

use super::entities::Observation;
use super::value_objects::{Labels, LlmGroup, LlmMeasure, LlmSignal};
use crate::catalogue::ProjectId;
use crate::explore::Window;
use crate::llm::{GroupBy, Spend, SpendGroup, SpendQuery};

const NANOS_PER_MILLI: f64 = 1_000_000.0;

impl LlmGroup {
    fn spend_group(self) -> GroupBy {
        match self {
            Self::None => GroupBy::Total,
            Self::Feature => GroupBy::Feature,
            Self::Customer => GroupBy::Customer,
            Self::Model => GroupBy::Model,
        }
    }
}

impl LlmSignal {
    pub fn query(&self, project: Option<ProjectId>, window: Window) -> SpendQuery {
        let mut query = SpendQuery::over(window);
        query.group_by = self.group_by.spend_group();
        query.filter.project = project.or(self.project);
        query.filter.feature.clone_from(&self.feature);
        query.filter.customer.clone_from(&self.customer);
        query.filter.model.clone_from(&self.model);
        query
    }

    pub fn observe(&self, spend: &Spend) -> Vec<Observation> {
        spend
            .groups
            .iter()
            .filter(|group| group.calls > 0)
            .map(|group| Observation {
                labels: self.labels_of(group),
                value: Some(self.value_of(group)),
            })
            .collect()
    }

    fn labels_of(&self, group: &SpendGroup) -> Labels {
        match self.group_by {
            LlmGroup::None => Labels::none(),
            named => Labels::new(BTreeMap::from([(named.as_str().into(), group.key.clone())])),
        }
    }

    fn value_of(&self, group: &SpendGroup) -> f64 {
        match self.measure {
            LlmMeasure::Cost => group.cost.dollars(),
            LlmMeasure::Tokens => group.input_tokens as f64 + group.output_tokens as f64,
            LlmMeasure::Calls => group.calls as f64,
            LlmMeasure::P95 => group.p95_nanos as f64 / NANOS_PER_MILLI,
        }
    }
}

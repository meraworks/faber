use crate::Project;
use faber_model::{Edit, Feature, Geometry, Scalar};
use faber_ports::Result;

impl Project {
    pub fn add_feature(
        &mut self,
        part: &str,
        feature: Feature<Scalar>,
        before: Option<&str>,
        make_output: bool,
    ) -> Result<()> {
        self.batch(vec![Edit::AddFeature {
            part: part.into(),
            feature,
            before: before.map(Into::into),
            make_output,
        }])
    }
    pub fn edit_feature(&mut self, part: &str, id: &str, geometry: Geometry<Scalar>) -> Result<()> {
        self.batch(vec![Edit::EditFeature {
            part: part.into(),
            id: id.into(),
            geometry,
        }])
    }
    pub fn delete_feature(&mut self, part: &str, id: &str) -> Result<()> {
        self.batch(vec![Edit::DeleteFeature {
            part: part.into(),
            id: id.into(),
        }])
    }
    pub fn move_feature(&mut self, part: &str, id: &str, before: Option<&str>) -> Result<()> {
        self.batch(vec![Edit::MoveFeature {
            part: part.into(),
            id: id.into(),
            before: before.map(Into::into),
        }])
    }
}

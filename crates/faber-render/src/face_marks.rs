use super::DrawList;
use std::{collections::BTreeMap, ops::Range};

type Face = (String, u32);

pub(super) struct FaceMarks {
    pub buffer: wgpu::Buffer,
    ranges: BTreeMap<Face, Vec<Range<u32>>>,
    current: BTreeMap<Face, u32>,
}

impl FaceMarks {
    pub fn new(
        device: &wgpu::Device,
        count: usize,
        ranges: BTreeMap<Face, Vec<Range<u32>>>,
    ) -> Self {
        Self {
            buffer: device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("Face highlight"),
                size: (count.max(1) * 4) as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            ranges,
            current: BTreeMap::new(),
        }
    }

    pub fn index(
        ranges: &mut BTreeMap<Face, Vec<Range<u32>>>,
        id: &str,
        start: u32,
        faces: &[u32],
    ) {
        let mut offset = 0;
        for run in faces.chunk_by(|a, b| a == b) {
            let end = offset + run.len() as u32 * 3;
            ranges
                .entry((id.to_owned(), run[0]))
                .or_default()
                .push(start + offset..start + end);
            offset = end;
        }
    }

    fn write(&self, queue: &wgpu::Queue, key: &Face, mark: u32) {
        if let Some(ranges) = self.ranges.get(key) {
            for range in ranges {
                queue.write_buffer(
                    &self.buffer,
                    range.start as u64 * 4,
                    bytemuck::cast_slice(&vec![mark; range.len()]),
                );
            }
        }
    }
}

impl DrawList {
    /// Change highlight values for the affected faces. Vertex positions stay on the GPU.
    pub fn mark_faces(
        &mut self,
        queue: &wgpu::Queue,
        selected: impl Iterator<Item = Face>,
        hover: Option<Face>,
    ) {
        let selected = hover
            .into_iter()
            .map(|key| (key, 1))
            .chain(selected.map(|key| (key, 2)))
            .collect::<BTreeMap<_, _>>();
        for key in self
            .marks
            .current
            .keys()
            .filter(|key| !selected.contains_key(*key))
        {
            self.marks.write(queue, key, 0);
        }
        for (key, &mark) in &selected {
            if self.marks.current.get(key) != Some(&mark) {
                self.marks.write(queue, key, mark);
            }
        }
        self.marks.current = selected;
    }
}

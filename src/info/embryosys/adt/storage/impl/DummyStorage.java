/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 */
package info.embryosys.adt.storage.impl;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtId;
import info.embryosys.adt.storage.Storage;

import java.util.Map;
import java.util.TreeMap;

/**
 * @author nrdufour
 * 
 */
public class DummyStorage implements Storage {

	private Map<AdtId, Adt> memoryStorage;

	/**
	 * 
	 */
	public DummyStorage() {
		this.memoryStorage = new TreeMap<AdtId, Adt>();
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.storage.Storage#clean(info.embryosys.adt.core.AdtId)
	 */
	@Override
	public void clean(AdtId id) {
		this.memoryStorage.remove(id);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.storage.Storage#load(info.embryosys.adt.core.AdtId)
	 */
	@Override
	public Adt load(AdtId id) {
		return this.memoryStorage.get(id);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see
	 * info.embryosys.adt.storage.Storage#store(info.embryosys.adt.core.Adt)
	 */
	@Override
	public void store(Adt adt) {
		this.memoryStorage.put(adt.getId(), adt);
	}

	/*
	 * (non-Javadoc)
	 * 
	 * @see info.embryosys.adt.storage.Storage#debugDump()
	 */
	@Override
	public void debugDump() {
		System.out.println("Dummy Storage Debug Dump:\n");
		for (AdtId id : this.memoryStorage.keySet()) {
			System.out.println("\t" + this.memoryStorage.get(id));
		}
	}

}

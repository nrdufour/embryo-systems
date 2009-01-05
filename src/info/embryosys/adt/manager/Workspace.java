/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.manager;

import info.embryosys.adt.storage.Storage;

/**
 * @author nrdufour
 * 
 */
public class Workspace {

	private final String name;
	private final Storage storage;

	/**
	 * 
	 */
	public Workspace(final String name, final Storage storage) {
		this.name = name;
		this.storage = storage;
	}

	/**
	 * @return the name
	 */
	public String getName() {
		return name;
	}

	/**
	 * @return the storage
	 */
	public Storage getStorage() {
		return storage;
	}

}

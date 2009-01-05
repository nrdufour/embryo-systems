/*
 * Embryo Systems
 *
 * $Id$
 *
 * Copyright 2009 Nicolas R. Dufour <nrdufour@gmail.com>
 * 
 * This source code is released under the BSD License.
 */
package info.embryosys.adt.storage;

import info.embryosys.adt.core.Adt;
import info.embryosys.adt.core.AdtId;
import info.embryosys.adt.core.AdtType;

/**
 * @author nrdufour
 * 
 */
public interface Storage {

	Adt load(AdtId id);

	void store(Adt adt);

	void clean(AdtId id);
	
	AdtId find(AdtType type, String name);
	
	void debugDump();
}
